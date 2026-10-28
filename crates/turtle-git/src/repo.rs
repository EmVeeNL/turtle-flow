//! A handle on one Git working directory (a repository or a job worktree). Every command runs
//! through [`turtle_exec`] with the hardened configuration from `docs/GIT-STRATEGY.md` §9.

use std::path::{Path, PathBuf};
use std::time::Duration;

use turtle_exec::{CommandSpec, ExitOutcome, Output};

use crate::{GitError, Result, git_command};

#[derive(Debug, Clone)]
pub struct Identity {
    pub name: String,
    pub email: String,
}

#[derive(Debug, Clone)]
pub struct Repo {
    cwd: PathBuf,
    env: Vec<(String, String)>,
}

impl Repo {
    #[must_use]
    pub fn new(cwd: impl Into<PathBuf>) -> Self {
        Self {
            cwd: cwd.into(),
            env: Vec::new(),
        }
    }

    /// Sets author and committer identity for commits created through this handle.
    #[must_use]
    pub fn with_identity(self, id: &Identity) -> Self {
        self.with_env("GIT_AUTHOR_NAME", &id.name)
            .with_env("GIT_AUTHOR_EMAIL", &id.email)
            .with_env("GIT_COMMITTER_NAME", &id.name)
            .with_env("GIT_COMMITTER_EMAIL", &id.email)
    }

    #[must_use]
    pub fn with_env(mut self, key: &str, value: &str) -> Self {
        self.env.push((key.to_owned(), value.to_owned()));
        self
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.cwd
    }

    /// A handle on another directory with the same environment (e.g. a worktree).
    #[must_use]
    pub fn at(&self, cwd: impl Into<PathBuf>) -> Self {
        Self {
            cwd: cwd.into(),
            env: self.env.clone(),
        }
    }

    pub(crate) fn spec<I, S>(&self, args: I) -> CommandSpec
    where
        I: IntoIterator<Item = S>,
        S: Into<std::ffi::OsString>,
    {
        let mut spec = git_command(args).cwd(&self.cwd);
        for (k, v) in &self.env {
            spec = spec.env(k, v);
        }
        spec
    }

    /// Runs git and returns the raw output without interpreting the exit status.
    pub(crate) async fn raw(&self, args: &[&str]) -> Result<Output> {
        Ok(turtle_exec::run(self.spec(args.iter().copied())).await?)
    }

    pub(crate) async fn raw_spec(&self, spec: CommandSpec) -> Result<Output> {
        Ok(turtle_exec::run(spec).await?)
    }

    fn fail(args: &[&str], out: &Output) -> GitError {
        GitError::Command {
            args: args.join(" "),
            code: match out.outcome {
                ExitOutcome::Exited(c) => c.to_string(),
                ExitOutcome::Signaled => "signal".into(),
                ExitOutcome::TimedOut => "timeout".into(),
                ExitOutcome::Cancelled => "cancelled".into(),
            },
            stderr: String::from_utf8_lossy(&out.stderr).trim().to_owned(),
        }
    }

    /// Runs git, requires exit 0, returns stdout with trailing whitespace trimmed.
    pub async fn run(&self, args: &[&str]) -> Result<String> {
        let out = self.raw(args).await?;
        if out.success() {
            Ok(out.stdout_lossy().trim_end().to_owned())
        } else {
            Err(Self::fail(args, &out))
        }
    }

    /// Runs git with stdin, requires exit 0.
    pub(crate) async fn run_stdin(&self, args: &[&str], stdin: &str) -> Result<String> {
        let out = self
            .raw_spec(
                self.spec(args.iter().copied())
                    .stdin(stdin.as_bytes().to_vec()),
            )
            .await?;
        if out.success() {
            Ok(out.stdout_lossy().trim_end().to_owned())
        } else {
            Err(Self::fail(args, &out))
        }
    }

    /// Exit status zero means true, one means false; anything else is an error.
    pub(crate) async fn run_bool(&self, args: &[&str]) -> Result<bool> {
        let out = self.raw(args).await?;
        match out.outcome {
            ExitOutcome::Exited(0) => Ok(true),
            ExitOutcome::Exited(1) => Ok(false),
            _ => Err(Self::fail(args, &out)),
        }
    }

    // ------------------------------------------------------------ queries

    /// True when `cwd` is inside a non-bare work tree.
    pub async fn is_work_tree(&self) -> bool {
        matches!(
            self.run(&["rev-parse", "--is-inside-work-tree"])
                .await
                .as_deref(),
            Ok("true")
        )
    }

    /// Validates that this is a usable project repository: non-bare, at least one commit,
    /// and not in the middle of a merge or rebase (FR-001).
    pub async fn validate_project_repo(&self) -> Result<()> {
        if self.run(&["rev-parse", "--is-bare-repository"]).await? == "true" {
            return Err(GitError::Invalid(
                "bare repositories are not supported".into(),
            ));
        }
        if !self.is_work_tree().await {
            return Err(GitError::Invalid("not a Git working tree".into()));
        }
        if self
            .run(&["rev-parse", "--verify", "-q", "HEAD"])
            .await
            .is_err()
        {
            return Err(GitError::Invalid("repository has no commits yet".into()));
        }
        let git_dir = self.git_dir().await?;
        for marker in [
            "MERGE_HEAD",
            "rebase-merge",
            "rebase-apply",
            "CHERRY_PICK_HEAD",
        ] {
            if git_dir.join(marker).exists() {
                return Err(GitError::Invalid(format!(
                    "repository has an unfinished operation ({marker}); finish or abort it first"
                )));
            }
        }
        Ok(())
    }

    pub async fn git_dir(&self) -> Result<PathBuf> {
        Ok(PathBuf::from(
            self.run(&["rev-parse", "--absolute-git-dir"]).await?,
        ))
    }

    /// The directory shared by all worktrees (`.git` of the main repository).
    pub async fn common_dir(&self) -> Result<PathBuf> {
        let raw = self.run(&["rev-parse", "--git-common-dir"]).await?;
        let p = PathBuf::from(raw);
        Ok(if p.is_absolute() { p } else { self.cwd.join(p) })
    }

    pub async fn head(&self) -> Result<String> {
        self.run(&["rev-parse", "--verify", "HEAD"]).await
    }

    pub async fn rev(&self, rev: &str) -> Result<String> {
        self.run(&["rev-parse", "--verify", &format!("{rev}^{{commit}}")])
            .await
    }

    /// Current branch name, or `None` for a detached HEAD.
    pub async fn current_branch(&self) -> Result<Option<String>> {
        let out = self.raw(&["symbolic-ref", "-q", "--short", "HEAD"]).await?;
        Ok(out
            .success()
            .then(|| out.stdout_lossy().trim().to_owned())
            .filter(|b| !b.is_empty()))
    }

    pub async fn branch_tip(&self, branch: &str) -> Result<Option<String>> {
        let out = self
            .raw(&[
                "rev-parse",
                "--verify",
                "-q",
                &format!("refs/heads/{branch}"),
            ])
            .await?;
        Ok(out.success().then(|| out.stdout_lossy().trim().to_owned()))
    }

    /// Fetches an `origin` branch into its remote-tracking ref. Repeating the operation is safe:
    /// Git updates the ref atomically, and callers can recover by reading the resulting tip.
    /// Returns `None` when there is no origin or that branch has not been published remotely.
    pub async fn fetch_origin_branch(&self, branch: &str) -> Result<Option<String>> {
        let branch_ref = format!("refs/heads/{branch}");
        if self.run(&["check-ref-format", &branch_ref]).await.is_err() {
            return Err(GitError::Invalid(format!(
                "invalid target branch `{branch}`"
            )));
        }
        let remotes = self.run(&["remote"]).await?;
        if !remotes.lines().any(|remote| remote == "origin") {
            return Ok(None);
        }

        let source = format!("refs/heads/{branch}");
        let destination = format!("refs/remotes/origin/{branch}");
        let refspec = format!("+{source}:{destination}");
        let args = [
            "fetch",
            "--no-tags",
            "--no-recurse-submodules",
            "--",
            "origin",
            refspec.as_str(),
        ];
        for attempt in 0..3 {
            let output = self
                .raw_spec(
                    self.spec(args.iter().copied())
                        .timeout(Duration::from_secs(30))
                        .max_output_bytes(64 * 1024),
                )
                .await?;
            if output.success() {
                return self
                    .run(&[
                        "rev-parse",
                        "--verify",
                        "-q",
                        &format!("{destination}^{{commit}}"),
                    ])
                    .await
                    .map(Some);
            }

            let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            if detail.contains("couldn't find remote ref")
                || detail.contains("could not find remote ref")
            {
                return Ok(None);
            }
            if attempt < 2 {
                tokio::time::sleep(Duration::from_millis(250 * (attempt + 1))).await;
            } else {
                return Err(GitError::RemoteFetch {
                    branch: branch.to_owned(),
                    attempts: 3,
                });
            }
        }
        unreachable!("all fetch outcomes return inside the bounded retry loop")
    }

    /// The default branch: `origin/HEAD` when set, else the current branch.
    pub async fn default_branch(&self) -> Result<String> {
        if let Ok(r) = self
            .run(&["symbolic-ref", "--short", "refs/remotes/origin/HEAD"])
            .await
            && let Some(name) = r.strip_prefix("origin/")
        {
            return Ok(name.to_owned());
        }
        self.current_branch()
            .await?
            .ok_or_else(|| GitError::Invalid("cannot determine the default branch".into()))
    }

    /// Porcelain v2 status entries (path per entry). Empty means a clean tree.
    pub async fn status_paths(&self) -> Result<Vec<String>> {
        let out = self
            .run(&["status", "--porcelain=v2", "-z", "--untracked-files=all"])
            .await?;
        let mut paths = Vec::new();
        let mut parts = out.split('\0').filter(|p| !p.is_empty());
        while let Some(entry) = parts.next() {
            match entry.chars().next() {
                Some('1' | 'u') => {
                    // 1: 8 fields then path; u: 10 fields then path
                    let skip = if entry.starts_with('u') { 10 } else { 8 };
                    paths.extend(entry.splitn(skip + 1, ' ').nth(skip).map(str::to_owned));
                }
                Some('2') => {
                    paths.extend(entry.splitn(10, ' ').nth(9).map(str::to_owned));
                    parts.next(); // original path of the rename
                }
                Some('?') => paths.extend(entry.strip_prefix("? ").map(str::to_owned)),
                _ => {}
            }
        }
        Ok(paths)
    }

    pub async fn is_clean(&self) -> Result<bool> {
        Ok(self.status_paths().await?.is_empty())
    }

    /// Branches checked out in any worktree of this repository (main checkout included).
    pub async fn checked_out_branches(&self) -> Result<Vec<String>> {
        let out = self.run(&["worktree", "list", "--porcelain"]).await?;
        Ok(out
            .lines()
            .filter_map(|l| l.strip_prefix("branch refs/heads/"))
            .map(str::to_owned)
            .collect())
    }

    pub async fn is_ancestor(&self, ancestor: &str, descendant: &str) -> Result<bool> {
        self.run_bool(&["merge-base", "--is-ancestor", ancestor, descendant])
            .await
    }
}
