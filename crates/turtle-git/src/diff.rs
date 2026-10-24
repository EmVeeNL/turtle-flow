//! Normalized diffs and their hashes: the acceptance approval binds `diff_sha256`
//! (`docs/WORKFLOW.md` §6.2). Normalization removes everything user or repo configuration could
//! change: no color, no external diff or textconv drivers, fixed algorithm.

use sha2::{Digest, Sha256};
use turtle_domain::Sha256 as Digest256;

use crate::{GitError, Repo, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedFile {
    pub path: String,
    /// `A`dded, `M`odified, `D`eleted, `R`enamed, `T` type change.
    pub status: char,
    pub old_path: Option<String>,
    /// `None` for binary files.
    pub added: Option<u64>,
    pub removed: Option<u64>,
}

impl Repo {
    /// The full binary-safe diff from `base` to `head`.
    pub async fn diff_bytes(&self, base: &str, head: &str) -> Result<Vec<u8>> {
        let out = self
            .raw(&[
                "diff",
                "--binary",
                "--full-index",
                "--no-color",
                "--no-ext-diff",
                "--no-textconv",
                "--diff-algorithm=histogram",
                &format!("{base}..{head}"),
            ])
            .await?;
        if out.success() {
            Ok(out.stdout)
        } else {
            Err(GitError::Command {
                args: "diff".into(),
                code: "nonzero".into(),
                stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
            })
        }
    }

    /// SHA-256 of [`Repo::diff_bytes`]: stable for identical content, different for any change.
    pub async fn diff_sha256(&self, base: &str, head: &str) -> Result<Digest256> {
        let bytes = self.diff_bytes(base, head).await?;
        Ok(Digest256::from_bytes(Sha256::digest(&bytes).into()))
    }

    /// Changed files with line counts, rename detection on.
    pub async fn changed_files(&self, base: &str, head: &str) -> Result<Vec<ChangedFile>> {
        let range = format!("{base}..{head}");
        let status = self
            .run(&["diff", "--name-status", "-z", "-M", "--no-ext-diff", &range])
            .await?;
        let numstat = self
            .run(&["diff", "--numstat", "-z", "-M", "--no-ext-diff", &range])
            .await?;
        let mut files = parse_name_status(&status)?;
        let counts = parse_numstat(&numstat)?;
        for f in &mut files {
            if let Some((a, r)) = counts
                .iter()
                .find(|(p, _, _)| *p == f.path)
                .map(|(_, a, r)| (*a, *r))
            {
                f.added = a;
                f.removed = r;
            }
        }
        Ok(files)
    }
}

pub(crate) fn parse_name_status(s: &str) -> Result<Vec<ChangedFile>> {
    let mut out = Vec::new();
    let mut parts = s.split('\0').filter(|p| !p.is_empty());
    while let Some(status) = parts.next() {
        let letter = status
            .chars()
            .next()
            .ok_or_else(|| GitError::Parse(status.into()))?;
        if letter == 'R' || letter == 'C' {
            let old = parts.next().ok_or_else(|| GitError::Parse(s.into()))?;
            let new = parts.next().ok_or_else(|| GitError::Parse(s.into()))?;
            out.push(ChangedFile {
                path: new.into(),
                status: 'R',
                old_path: Some(old.into()),
                added: None,
                removed: None,
            });
        } else {
            let path = parts.next().ok_or_else(|| GitError::Parse(s.into()))?;
            out.push(ChangedFile {
                path: path.into(),
                status: letter,
                old_path: None,
                added: None,
                removed: None,
            });
        }
    }
    Ok(out)
}

/// `-z` numstat: `added\tremoved\tpath\0`, or `added\tremoved\t\0old\0new\0` for renames.
#[allow(clippy::type_complexity)]
pub(crate) fn parse_numstat(s: &str) -> Result<Vec<(String, Option<u64>, Option<u64>)>> {
    let mut out = Vec::new();
    let mut parts = s.split('\0');
    while let Some(head) = parts.next() {
        if head.is_empty() {
            continue;
        }
        let mut f = head.splitn(3, '\t');
        let (a, r, path) = (f.next(), f.next(), f.next());
        let (Some(a), Some(r), Some(path)) = (a, r, path) else {
            return Err(GitError::Parse(head.into()));
        };
        let num = |x: &str| if x == "-" { None } else { x.parse().ok() };
        let path = if path.is_empty() {
            let _old = parts.next();
            parts
                .next()
                .ok_or_else(|| GitError::Parse(s.into()))?
                .to_owned()
        } else {
            path.to_owned()
        };
        out.push((path, num(a), num(r)));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_name_status_with_rename() {
        let f = parse_name_status("M\0src/a.rs\0R100\0old.rs\0new.rs\0A\0b.rs\0").expect("parse");
        assert_eq!(f.len(), 3);
        assert_eq!((f[0].status, f[0].path.as_str()), ('M', "src/a.rs"));
        assert_eq!(
            (f[1].status, f[1].old_path.as_deref(), f[1].path.as_str()),
            ('R', Some("old.rs"), "new.rs")
        );
    }

    #[test]
    fn parses_numstat_with_rename_and_binary() {
        let n = parse_numstat("3\t1\tsrc/a.rs\0-\t-\timg.png\0\x30\t0\t\0old.rs\0new.rs\0")
            .expect("parse");
        assert_eq!(n[0], ("src/a.rs".into(), Some(3), Some(1)));
        assert_eq!(n[1], ("img.png".into(), None, None));
        assert_eq!(n[2], ("new.rs".into(), Some(0), Some(0)));
    }
}
