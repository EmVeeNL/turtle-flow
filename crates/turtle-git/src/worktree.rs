//! Worktree lifecycle (`docs/GIT-STRATEGY.md` §3): one worktree and one branch per job,
//! created idempotently so a crash between "intent recorded" and "Git done" is recoverable.

use std::path::{Component, Path, PathBuf};

use crate::names::{check_ref_format, is_valid_job_key};
use crate::{GitError, Repo, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeSpec {
    /// Directory holding all factory worktrees, e.g. `~/.turtle/worktrees`.
    pub root: PathBuf,
    pub project_slug: String,
    pub job_key: String,
    pub branch: String,
    pub base_commit: String,
}

impl WorktreeSpec {
    /// `<root>/<project-slug>/<job-key>`. Both parts are validated so the path cannot escape
    /// the worktree root (no `..`, separators or odd characters).
    pub fn path(&self) -> Result<PathBuf> {
        let slug_ok = !self.project_slug.is_empty()
            && self
                .project_slug
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        if !slug_ok {
            return Err(GitError::Invalid(format!(
                "invalid project slug `{}`",
                self.project_slug
            )));
        }
        if !is_valid_job_key(&self.job_key) {
            return Err(GitError::Invalid(format!(
                "invalid job key `{}`",
                self.job_key
            )));
        }
        let path = self.root.join(&self.project_slug).join(&self.job_key);
        debug_assert!(
            path.components()
                .all(|c| !matches!(c, Component::ParentDir))
        );
        Ok(path)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorktreeOutcome {
    Created,
    /// Already present at the expected branch and base: nothing to do (recovery probe).
    AlreadyPresent,
}

impl Repo {
    /// Creates the job worktree and branch at `spec.base_commit`, or confirms they exist.
    pub async fn ensure_worktree(&self, spec: &WorktreeSpec) -> Result<WorktreeOutcome> {
        let path = spec.path()?;
        check_ref_format(self, &spec.branch).await?;
        let base = self.rev(&spec.base_commit).await?;

        if path.exists() {
            let wt = self.at(&path);
            let on_branch =
                wt.current_branch().await.ok().flatten().as_deref() == Some(spec.branch.as_str());
            if on_branch && self.is_ancestor(&base, &spec.branch).await? {
                return Ok(WorktreeOutcome::AlreadyPresent);
            }
            return Err(GitError::Invalid(format!(
                "{} exists but is not the expected worktree for {}",
                path.display(),
                spec.branch
            )));
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let path_str = path.to_string_lossy();
        match self.branch_tip(&spec.branch).await? {
            // A previous attempt created the branch but not the worktree: reuse it if it is
            // still at the base; never move an existing branch.
            Some(tip) if tip == base => {
                self.run(&["worktree", "add", &path_str, &spec.branch])
                    .await?;
            }
            Some(tip) => {
                return Err(GitError::Invalid(format!(
                    "branch {} already exists at {tip}, expected {base}",
                    spec.branch
                )));
            }
            None => {
                self.run(&["worktree", "add", "-b", &spec.branch, &path_str, &base])
                    .await?;
            }
        }
        Ok(WorktreeOutcome::Created)
    }

    /// Removes a worktree (force only discards ignored or untracked leftovers) and prunes.
    pub async fn remove_worktree(&self, path: &Path, force: bool) -> Result<()> {
        let path_str = path.to_string_lossy();
        if path.exists() {
            let mut args = vec!["worktree", "remove"];
            if force {
                args.push("--force");
            }
            args.push(&path_str);
            self.run(&args).await?;
        }
        self.run(&["worktree", "prune"]).await?;
        Ok(())
    }

    /// Deletes a branch only if it matches the factory prefix and is fully merged or forced.
    pub async fn delete_branch(&self, branch: &str, prefix: &str, force: bool) -> Result<()> {
        if !branch.starts_with(prefix) {
            return Err(GitError::Invalid(format!(
                "refusing to delete non-factory branch `{branch}`"
            )));
        }
        self.run(&["branch", if force { "-D" } else { "-d" }, branch])
            .await?;
        Ok(())
    }
}
