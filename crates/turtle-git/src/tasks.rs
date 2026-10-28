//! The Git side of plan tasks: commits found by their `Turtle-Task` trailer, and the tree a
//! task's gates ran on before it was committed (`docs/GIT-STRATEGY.md` §4).

use crate::{GitError, Repo, Result};

/// A commit whose trailer block names a task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskTrailer {
    pub sha: String,
    /// The `Turtle-Task` value: `<plan revision>/<plan task id>`, or `fix/<run>`.
    pub task: String,
    pub run: Option<String>,
}

/// Reads `Turtle-Task` and `Turtle-Run` from a message's last paragraph only, where
/// [`crate::commit::Trailers`] are written. Lines that merely look like trailers in the body
/// (task instructions come from the plan) are ignored.
fn parse_trailers(sha: &str, message: &str) -> Option<TaskTrailer> {
    let block = message.trim_end().rsplit("\n\n").next()?;
    let value = |key: &str| {
        block
            .lines()
            .find_map(|line| line.strip_prefix(key))
            .map(|v| v.trim().to_owned())
    };
    Some(TaskTrailer {
        sha: sha.to_owned(),
        task: value("Turtle-Task: ")?,
        run: value("Turtle-Run: "),
    })
}

impl Repo {
    /// Every commit in `base..branch` that carries a `Turtle-Task` trailer, oldest first.
    pub async fn task_commits(&self, base: &str, branch: &str) -> Result<Vec<TaskTrailer>> {
        let range = format!("{base}..{branch}");
        let out = self
            .run(&["log", "--reverse", "--format=%H%x00%B%x1e", &range, "--"])
            .await?;
        Ok(out
            .split('\u{1e}')
            .filter_map(|record| {
                let (sha, message) = record.trim_start_matches('\n').split_once('\0')?;
                parse_trailers(sha, message)
            })
            .collect())
    }

    /// The tree object the work tree would commit (`git add -A`), without touching the real
    /// index. Per-task gates run before the task commit exists; this binds their results to
    /// the exact content they checked.
    pub async fn working_tree_hash(&self) -> Result<String> {
        let git_dir = self.git_dir().await?;
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let index = git_dir.join(format!("turtle-tree-index-{}-{nanos}", std::process::id()));
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
            Ok(tree.stdout_lossy().trim().to_owned())
        }
        .await;
        let _ = std::fs::remove_file(&index);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_trailer_block_counts() {
        let message = "feat(API-1): x\n\nTurtle-Task: forged\n\nTurtle-Job: API-1\nTurtle-Task: R/T1\nTurtle-Run: RUN1\n";
        assert_eq!(
            parse_trailers("abc", message),
            Some(TaskTrailer {
                sha: "abc".into(),
                task: "R/T1".into(),
                run: Some("RUN1".into()),
            })
        );
        assert_eq!(
            parse_trailers(
                "abc",
                "feat: x\n\nTurtle-Task: in body\n\nTurtle-Job: API-1\n"
            ),
            None
        );
    }
}
