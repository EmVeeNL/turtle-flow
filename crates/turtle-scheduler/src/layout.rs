//! Where a job's files live, and the configuration the executors share.

use std::path::PathBuf;
use std::time::Duration;

use turtle_git::names::{DEFAULT_BRANCH_PREFIX, job_branch_name};
use turtle_git::worktree::WorktreeSpec;
use turtle_git::{Identity, Repo};
use turtle_store::{JobRecord, ProjectRecord};

#[derive(Debug, Clone)]
pub struct ExecConfig {
    /// Directory holding all job worktrees, e.g. `~/.turtle/worktrees`.
    pub worktree_root: PathBuf,
    pub default_branch_prefix: String,
    /// Identity for commits made by Turtle Flow; `None` uses the Git configuration.
    pub identity: Option<Identity>,
    /// Extra environment for every Git command (tests use it for hermetic configuration).
    pub git_env: Vec<(String, String)>,
    pub default_gate_timeout: Duration,
    /// `gh` executable; tests can point this at a stand-in script.
    pub gh_program: PathBuf,
}

impl ExecConfig {
    #[must_use]
    pub fn new(worktree_root: impl Into<PathBuf>) -> Self {
        Self {
            worktree_root: worktree_root.into(),
            default_branch_prefix: DEFAULT_BRANCH_PREFIX.to_owned(),
            identity: None,
            git_env: Vec::new(),
            default_gate_timeout: Duration::from_secs(20 * 60),
            gh_program: PathBuf::from("gh"),
        }
    }

    /// A Git handle on the project's main repository.
    #[must_use]
    pub fn repo(&self, project: &ProjectRecord) -> Repo {
        let mut repo = Repo::new(&project.repo_path);
        if let Some(id) = &self.identity {
            repo = repo.with_identity(id);
        }
        for (k, v) in &self.git_env {
            repo = repo.with_env(k, v);
        }
        repo
    }
}

/// Everything about a job's branch and worktree, derived deterministically so that a restart
/// finds exactly what an earlier attempt created.
#[derive(Debug, Clone)]
pub struct Layout {
    pub spec: WorktreeSpec,
    pub path: PathBuf,
    pub target_branch: String,
}

impl Layout {
    pub fn new(
        cfg: &ExecConfig,
        project: &ProjectRecord,
        job: &JobRecord,
        base_commit: &str,
    ) -> Result<Self, turtle_git::GitError> {
        let prefix = project
            .settings
            .branch_prefix
            .as_deref()
            .unwrap_or(&cfg.default_branch_prefix);
        let spec = WorktreeSpec {
            root: cfg.worktree_root.clone(),
            project_slug: project.slug.clone(),
            job_key: job.key.to_string(),
            branch: job_branch_name(prefix, &job.key.to_string(), &job.title)?,
            base_commit: base_commit.to_owned(),
        };
        let path = spec.path()?;
        let target_branch = project
            .settings
            .target_branch
            .clone()
            .unwrap_or_else(|| project.default_branch.clone());
        Ok(Self {
            spec,
            path,
            target_branch,
        })
    }
}
