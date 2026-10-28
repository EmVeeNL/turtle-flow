//! Where Turtle Flow keeps its files: `$TURTLE_HOME`, or `~/.turtle`.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

#[derive(Debug, Clone)]
pub struct Home {
    pub root: PathBuf,
}

impl Home {
    /// Resolves the data directory without creating it.
    pub fn locate() -> Result<Self> {
        let root = std::env::var_os("TURTLE_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".turtle")))
            .context("neither TURTLE_HOME nor HOME is set")?;
        Ok(Self { root })
    }

    #[must_use]
    pub fn db(&self) -> PathBuf {
        self.root.join("turtle.db")
    }

    #[must_use]
    pub fn blobs(&self) -> PathBuf {
        self.root.join("blobs")
    }

    #[must_use]
    pub fn worktrees(&self) -> PathBuf {
        self.root.join("worktrees")
    }

    #[must_use]
    pub fn daemon_lock(&self) -> PathBuf {
        self.root.join("daemon.lock")
    }

    #[must_use]
    pub fn daemon_pid(&self) -> PathBuf {
        self.root.join("daemon.pid")
    }

    #[must_use]
    pub fn daemon_log(&self) -> PathBuf {
        self.root.join("daemon.log")
    }

    /// Creates the directory with mode `0700` (SEC-013) and refuses one others can enter.
    pub fn ensure(&self) -> Result<()> {
        std::fs::create_dir_all(&self.root)
            .with_context(|| format!("creating {}", self.root.display()))?;
        restrict(&self.root)
    }
}

#[cfg(unix)]
fn restrict(dir: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(dir)?.permissions().mode() & 0o777;
    if mode & 0o077 != 0 {
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
            .with_context(|| format!("restricting {}", dir.display()))?;
        if std::fs::metadata(dir)?.permissions().mode() & 0o077 != 0 {
            bail!("{} is accessible by other users", dir.display());
        }
    }
    Ok(())
}

#[cfg(not(unix))]
fn restrict(_dir: &Path) -> Result<()> {
    Ok(())
}
