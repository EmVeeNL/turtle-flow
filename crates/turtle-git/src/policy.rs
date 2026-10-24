//! Post-run Git policy check (FR-065, FR-093, SEC-014). Agents may edit files but must not
//! commit, switch branches, move refs, or touch Git configuration and hooks. The factory takes a
//! snapshot before a run and verifies it afterwards; any difference fails the run.

use std::path::Path;

use sha2::{Digest, Sha256};

use crate::{Repo, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitSnapshot {
    pub head: String,
    pub branch: Option<String>,
    refs_hash: String,
    git_dir_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitPolicyCheck {
    pub head_unchanged: bool,
    pub branch_unchanged: bool,
    pub refs_unchanged: bool,
    pub git_dir_untouched: bool,
}

impl GitPolicyCheck {
    #[must_use]
    pub const fn is_ok(&self) -> bool {
        self.head_unchanged
            && self.branch_unchanged
            && self.refs_unchanged
            && self.git_dir_untouched
    }

    /// Human-readable list of what was violated.
    #[must_use]
    pub fn violations(&self) -> Vec<&'static str> {
        let mut v = Vec::new();
        if !self.head_unchanged {
            v.push("HEAD moved (the agent committed or reset)");
        }
        if !self.branch_unchanged {
            v.push("branch changed (the agent switched branches)");
        }
        if !self.refs_unchanged {
            v.push("refs changed (branches, tags or other refs were created, moved or deleted)");
        }
        if !self.git_dir_untouched {
            v.push("Git configuration or hooks were modified");
        }
        v
    }
}

impl Repo {
    pub async fn snapshot(&self) -> Result<GitSnapshot> {
        let refs = self
            .run(&["for-each-ref", "--format=%(refname) %(objectname)"])
            .await?;
        Ok(GitSnapshot {
            head: self.head().await?,
            branch: self.current_branch().await?,
            refs_hash: hex(Sha256::digest(refs.as_bytes())),
            git_dir_hash: self.git_dir_hash().await?,
        })
    }

    pub async fn check_policy(&self, before: &GitSnapshot) -> Result<GitPolicyCheck> {
        let now = self.snapshot().await?;
        Ok(GitPolicyCheck {
            head_unchanged: now.head == before.head,
            branch_unchanged: now.branch == before.branch,
            refs_unchanged: now.refs_hash == before.refs_hash,
            git_dir_untouched: now.git_dir_hash == before.git_dir_hash,
        })
    }

    /// Hash of the shared `config`, this worktree's `config.worktree`, and all non-sample hooks.
    async fn git_dir_hash(&self) -> Result<String> {
        let common = self.common_dir().await?;
        let own = self.git_dir().await?;
        let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
        for (label, file) in [
            ("config", common.join("config")),
            ("config.worktree", own.join("config.worktree")),
        ] {
            entries.push((label.into(), std::fs::read(&file).unwrap_or_default()));
        }
        collect_hooks(&common.join("hooks"), "hooks", &mut entries);
        entries.sort();
        let mut h = Sha256::new();
        for (name, bytes) in entries {
            h.update(name.as_bytes());
            h.update([0]);
            h.update(Sha256::digest(&bytes));
        }
        Ok(hex(h.finalize()))
    }
}

fn collect_hooks(dir: &Path, prefix: &str, out: &mut Vec<(String, Vec<u8>)>) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in read.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.ends_with(".sample") {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            collect_hooks(&path, &format!("{prefix}/{name}"), out);
        } else {
            out.push((
                format!("{prefix}/{name}"),
                std::fs::read(&path).unwrap_or_default(),
            ));
        }
    }
}

fn hex(bytes: impl AsRef<[u8]>) -> String {
    bytes.as_ref().iter().map(|b| format!("{b:02x}")).collect()
}
