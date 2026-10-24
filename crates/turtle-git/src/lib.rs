//! Typed wrapper around the Git CLI. All invocations use the hardened base configuration
//! from `docs/GIT-STRATEGY.md` §9 (hooks and fsmonitor disabled, restricted protocols).

use std::time::Duration;

use turtle_exec::{CommandSpec, ExecError};

/// Minimum Git version: `merge-tree --write-tree` (non-mutating conflict probes) needs 2.38.
pub const MIN_GIT_VERSION: semver::Version = semver::Version::new(2, 38, 0);

/// Hardening flags prepended to every Git invocation.
pub const HARDENED_CONFIG: &[&str] = &[
    "-c",
    "core.hooksPath=/dev/null",
    "-c",
    "core.fsmonitor=false",
    "-c",
    "protocol.file.allow=user",
    "-c",
    "protocol.ext.allow=never",
    "-c",
    "submodule.recurse=false",
    "-c",
    "gc.auto=0",
];

#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error(transparent)]
    Exec(#[from] ExecError),
    #[error("git exited unsuccessfully: {stderr}")]
    Failed { stderr: String },
    #[error("git {args} failed (exit {code}): {stderr}")]
    Command {
        args: String,
        code: String,
        stderr: String,
    },
    #[error("could not parse git version from {0:?}")]
    VersionParse(String),
    #[error("invalid input: {0}")]
    Invalid(String),
    #[error("unexpected git output: {0}")]
    Parse(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("commit refused: {0}")]
    CommitRefused(String),
    #[error("target branch `{branch}` moved since the delivery was prepared")]
    TargetMoved { branch: String },
    #[error("merge has conflicts in {} file(s)", .0.len())]
    Conflicts(Vec<String>),
}

pub type Result<T, E = GitError> = std::result::Result<T, E>;

pub mod commit;
pub mod delivery;
pub mod diff;
pub mod merge;
pub mod names;
pub mod policy;
pub mod recovery;
pub mod repo;
pub mod worktree;

pub use repo::{Identity, Repo};

/// Builds a hardened `git` command.
#[must_use]
pub fn git_command<I, S>(args: I) -> CommandSpec
where
    I: IntoIterator<Item = S>,
    S: Into<std::ffi::OsString>,
{
    CommandSpec::new("git")
        .args(HARDENED_CONFIG.iter().copied())
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .timeout(Duration::from_secs(60))
}

/// Parses `git version 2.54.0 (Apple Git-157)` and similar outputs.
pub fn parse_version(output: &str) -> Result<semver::Version, GitError> {
    let raw = output
        .split_whitespace()
        .nth(2)
        .ok_or_else(|| GitError::VersionParse(output.to_owned()))?;
    // Git versions may have 4 components (e.g. 2.39.3.windows.1); keep major.minor.patch.
    let mut parts = raw.split('.').map(str::parse::<u64>);
    match (parts.next(), parts.next(), parts.next()) {
        (Some(Ok(major)), Some(Ok(minor)), Some(Ok(patch))) => {
            Ok(semver::Version::new(major, minor, patch))
        }
        (Some(Ok(major)), Some(Ok(minor)), _) => Ok(semver::Version::new(major, minor, 0)),
        _ => Err(GitError::VersionParse(output.to_owned())),
    }
}

/// Returns the installed Git version.
pub async fn version() -> Result<semver::Version, GitError> {
    let out = turtle_exec::run(git_command(["--version"])).await?;
    if !out.success() {
        return Err(GitError::Failed {
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        });
    }
    parse_version(out.stdout_lossy().trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_common_version_strings() {
        assert_eq!(
            parse_version("git version 2.54.0 (Apple Git-157)").expect("apple"),
            semver::Version::new(2, 54, 0)
        );
        assert_eq!(
            parse_version("git version 2.39.3.windows.1").expect("windows"),
            semver::Version::new(2, 39, 3)
        );
        assert_eq!(
            parse_version("git version 2.40").expect("two parts"),
            semver::Version::new(2, 40, 0)
        );
        assert!(parse_version("nonsense").is_err());
    }

    #[test]
    fn hardened_config_disables_hooks() {
        let spec = git_command(["status"]);
        let args: Vec<_> = spec.args.iter().map(|a| a.to_string_lossy()).collect();
        assert!(args.contains(&"core.hooksPath=/dev/null".into()));
        assert_eq!(args.last().map(AsRef::as_ref), Some("status"));
    }

    #[tokio::test]
    async fn reads_installed_version() {
        let v = version().await.expect("git installed in dev and CI");
        assert!(v.major >= 2);
    }
}
