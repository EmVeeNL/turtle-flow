//! Conflict-free refresh of a job worktree when its target branch has moved.

use crate::merge::MergeProbe;
use crate::{GitError, Repo, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefreshOutcome {
    Refreshed { head: String },
    Conflicts { files: Vec<String> },
}

impl Repo {
    /// Replays the job's final tree on `new_base` as one factory commit. A retry recognizes a
    /// worktree already refreshed before the database was updated.
    pub async fn refresh_job(
        &self,
        worktree: &Repo,
        old_base: &str,
        new_base: &str,
        job_key: &str,
    ) -> Result<RefreshOutcome> {
        if !worktree.is_clean().await? {
            return Err(GitError::Invalid(
                "job worktree has uncommitted changes; commit or discard them before refresh"
                    .into(),
            ));
        }
        let head = worktree.head().await?;
        if self.is_ancestor(new_base, &head).await? {
            return Ok(RefreshOutcome::Refreshed { head });
        }
        if !self.is_ancestor(old_base, &head).await? {
            return Err(GitError::Invalid(
                "job branch no longer contains its recorded base; resolve the branch manually"
                    .into(),
            ));
        }
        let tree = match self.probe_merge(new_base, &head).await? {
            MergeProbe::Clean { tree } => tree,
            MergeProbe::Conflicts { files } => return Ok(RefreshOutcome::Conflicts { files }),
        };
        let message =
            format!("chore({job_key}): refresh onto updated target\n\nTurtle-Job: {job_key}\n");
        let refreshed = self
            .run_stdin(&["commit-tree", &tree, "-p", new_base, "-F", "-"], &message)
            .await?;
        worktree.run(&["reset", "--hard", &refreshed]).await?;
        Ok(RefreshOutcome::Refreshed { head: refreshed })
    }
}
