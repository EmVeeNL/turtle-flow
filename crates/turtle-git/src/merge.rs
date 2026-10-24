//! Non-mutating conflict probes with `git merge-tree --write-tree` (Git ≥ 2.38).
//! `docs/GIT-STRATEGY.md` §7.

use turtle_exec::ExitOutcome;

use crate::{GitError, Repo, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergeProbe {
    /// The merge would succeed; `tree` is the resulting tree object.
    Clean {
        tree: String,
    },
    Conflicts {
        files: Vec<String>,
    },
}

impl Repo {
    /// Simulates merging `theirs` into `ours` without touching any working tree or ref.
    pub async fn probe_merge(&self, ours: &str, theirs: &str) -> Result<MergeProbe> {
        let args = [
            "merge-tree",
            "--write-tree",
            "--name-only",
            "--no-messages",
            ours,
            theirs,
        ];
        let out = self.raw(&args).await?;
        let text = out.stdout_lossy();
        let mut lines = text.lines();
        match out.outcome {
            ExitOutcome::Exited(0) => {
                let tree = lines
                    .next()
                    .ok_or_else(|| GitError::Parse("empty merge-tree output".into()))?;
                Ok(MergeProbe::Clean {
                    tree: tree.trim().to_owned(),
                })
            }
            ExitOutcome::Exited(1) => {
                // First line is the (conflicted) tree; then conflicted paths until a blank line.
                let _tree = lines.next();
                let mut files: Vec<String> = lines
                    .take_while(|l| !l.is_empty())
                    .map(str::to_owned)
                    .collect();
                files.sort();
                files.dedup();
                Ok(MergeProbe::Conflicts { files })
            }
            _ => Err(GitError::Command {
                args: args.join(" "),
                code: format!("{:?}", out.outcome),
                stderr: String::from_utf8_lossy(&out.stderr).trim().to_owned(),
            }),
        }
    }
}
