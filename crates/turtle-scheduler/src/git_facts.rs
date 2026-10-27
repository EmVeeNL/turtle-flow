//! The real [`GitFactsProvider`]: reads the job branch to bind acceptance approvals to the exact
//! diff, and derives the flags a human must acknowledge (protected paths, dependency changes).
//!
//! Any error yields `None`, which makes acceptance impossible: failing to read the Git state
//! must never produce an approvable (or approved) state.

use std::sync::Arc;

use turtle_domain::paths::{DEFAULT_PROTECTED_PATHS, flags_for_changes};
use turtle_policy::GitBinding;
use turtle_store::JobRecord;

use crate::binding::{GitFacts, GitFactsProvider};
use crate::layout::{ExecConfig, Layout};
use crate::shared::SharedStore;

pub struct RepoGitFacts {
    store: SharedStore,
    cfg: Arc<ExecConfig>,
}

impl std::fmt::Debug for RepoGitFacts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RepoGitFacts").finish_non_exhaustive()
    }
}

impl RepoGitFacts {
    #[must_use]
    pub fn new(store: SharedStore, cfg: Arc<ExecConfig>) -> Self {
        Self { store, cfg }
    }
}

impl GitFactsProvider for RepoGitFacts {
    async fn facts(&self, job: &JobRecord) -> Option<GitFacts> {
        let base = job.base_commit.clone()?;
        let id = job.job.id;
        let project = self.store.with(move |s| s.project_of_job(id)).await.ok()?;
        let layout = Layout::new(&self.cfg, &project, job, &base).ok()?;
        let repo = self.cfg.repo(&project);

        let head = repo.branch_tip(&layout.spec.branch).await.ok()??;
        let changed = repo.changed_files(&base, &head).await.ok()?;
        let diff_sha256 = repo.diff_sha256(&base, &head).await.ok()?;

        let protected: Vec<String> =
            project.settings.protected_paths.clone().unwrap_or_else(|| {
                DEFAULT_PROTECTED_PATHS
                    .iter()
                    .map(ToString::to_string)
                    .collect()
            });
        let mut paths: Vec<String> = changed.iter().map(|c| c.path.clone()).collect();
        paths.extend(changed.iter().filter_map(|c| c.old_path.clone()));
        let mut required_flags = flags_for_changes(&paths, &protected);
        if !project.settings.static_gates.protected_paths {
            required_flags.retain(|flag| !flag.starts_with("protected_path:"));
        }
        if !project.settings.static_gates.dependency_changes {
            required_flags.retain(|flag| !flag.starts_with("dependency_manifest:"));
        }
        Some(GitFacts {
            binding: GitBinding {
                base_commit: base,
                head_commit: head,
                diff_sha256,
                target_branch: layout.target_branch,
            },
            required_flags,
        })
    }
}
