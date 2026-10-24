//! Crash and interruption recovery for worktrees (`docs/WORKFLOW.md` §10, GIT-STRATEGY §10).

use std::time::{Duration, SystemTime};

use crate::{GitError, Repo, Result};

/// A stale lock younger than this might belong to a git process that just started.
pub const STALE_LOCK_AGE: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Healed {
    RemovedStaleIndexLock,
    /// `index.lock` exists but is too young or a git process is running; left alone.
    LeftIndexLock,
    AbortedRebase,
    AbortedMerge,
}

impl Repo {
    /// Saves uncommitted work (tracked, modified and untracked, ignoring ignored files) to
    /// `refs/turtle/recovery/<run>` and resets the worktree to its last commit. Returns the
    /// recovery commit, or `None` if the tree was clean.
    pub async fn stash_to_recovery_ref(&self, run_id: &str) -> Result<Option<String>> {
        if run_id.is_empty()
            || !run_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return Err(GitError::Invalid(format!("invalid run id `{run_id}`")));
        }
        if self.is_clean().await? {
            return Ok(None);
        }
        // Build the snapshot in a temporary index so the real index is never disturbed.
        let git_dir = self.git_dir().await?;
        let index = git_dir.join(format!("turtle-recovery-index-{run_id}"));
        let with_index = |args: &[&str]| {
            self.spec(args.iter().copied())
                .env("GIT_INDEX_FILE", index.to_string_lossy())
        };
        let result = async {
            for args in [&["read-tree", "HEAD"][..], &["add", "-A"][..]] {
                let out = self.raw_spec(with_index(args)).await?;
                if !out.success() {
                    return Err(GitError::Command {
                        args: args.join(" "),
                        code: "nonzero".into(),
                        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
                    });
                }
            }
            let tree = self.raw_spec(with_index(&["write-tree"])).await?;
            if !tree.success() {
                return Err(GitError::Parse("write-tree failed".into()));
            }
            let tree = tree.stdout_lossy().trim().to_owned();
            let msg = format!("turtle recovery snapshot for run {run_id}\n");
            let commit = self
                .run_stdin(&["commit-tree", &tree, "-p", "HEAD", "-F", "-"], &msg)
                .await?;
            self.run(&[
                "update-ref",
                &format!("refs/turtle/recovery/{run_id}"),
                &commit,
            ])
            .await?;
            Ok(commit)
        }
        .await;
        let _ = std::fs::remove_file(&index);
        let commit = result?;
        self.run(&["reset", "--hard", "-q", "HEAD"]).await?;
        self.run(&["clean", "-fdq"]).await?;
        Ok(Some(commit))
    }

    /// Repairs interrupted operations in a factory worktree. Never touches the user's checkout:
    /// callers must only pass worktrees created by Turtle Flow.
    pub async fn heal(&self) -> Result<Vec<Healed>> {
        self.heal_with(git_process_running().await).await
    }

    /// Like [`Repo::heal`] with the "is a git process running" answer supplied by the caller.
    pub async fn heal_with(&self, git_running: bool) -> Result<Vec<Healed>> {
        let git_dir = self.git_dir().await?;
        let mut healed = Vec::new();

        let lock = git_dir.join("index.lock");
        if let Ok(meta) = std::fs::metadata(&lock) {
            let age = meta
                .modified()
                .ok()
                .and_then(|m| SystemTime::now().duration_since(m).ok())
                .unwrap_or_default();
            if age >= STALE_LOCK_AGE && !git_running {
                std::fs::remove_file(&lock)?;
                healed.push(Healed::RemovedStaleIndexLock);
            } else {
                healed.push(Healed::LeftIndexLock);
            }
        }
        if git_dir.join("rebase-merge").exists() || git_dir.join("rebase-apply").exists() {
            self.run(&["rebase", "--abort"]).await?;
            healed.push(Healed::AbortedRebase);
        }
        if git_dir.join("MERGE_HEAD").exists() {
            self.run(&["merge", "--abort"]).await?;
            healed.push(Healed::AbortedMerge);
        }
        Ok(healed)
    }
}

/// Conservative check: any running `git` process anywhere blocks lock removal.
/// A false positive only delays recovery; a false negative could corrupt a live operation.
async fn git_process_running() -> bool {
    let spec = turtle_exec::CommandSpec::new("ps")
        .args(["-axo", "comm="])
        .timeout(Duration::from_secs(5));
    match turtle_exec::run(spec).await {
        Ok(out) if out.success() => out
            .stdout_lossy()
            .lines()
            .any(|l| l.trim() == "git" || l.trim().ends_with("/git")),
        // If we cannot tell, assume one is running.
        _ => true,
    }
}
