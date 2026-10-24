//! Commits created by Turtle Flow (agents never commit). `docs/GIT-STRATEGY.md` §4.

use std::path::{Component, Path};

use crate::{GitError, Repo, Result};

/// Single files above this size are refused unless the policy allows them.
pub const MAX_FILE_BYTES: u64 = 5 * 1024 * 1024;

/// Files that must never be committed by an agent run (SEC-003, `docs/SECURITY.md` §6.3).
#[derive(Debug, Clone)]
pub struct NeverInclude;

impl NeverInclude {
    #[must_use]
    pub fn matches(path: &str) -> bool {
        let name = path.rsplit('/').next().unwrap_or(path);
        let lower = name.to_ascii_lowercase();
        lower.starts_with(".env")
            || [".pem", ".key", ".p12", ".keystore", ".tfstate"]
                .iter()
                .any(|ext| lower.ends_with(ext))
            || lower.starts_with("id_")
            || matches!(lower.as_str(), ".npmrc" | ".pypirc")
            || path.split('/').any(|c| c == "secrets")
            || path.split('/').any(|c| c == ".git")
    }
}

/// Trailers linking a commit to its job, task, run and model (FR-092).
#[derive(Debug, Clone, Default)]
pub struct Trailers {
    pub job: String,
    pub task: Option<String>,
    pub run: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub context: Option<String>,
}

impl Trailers {
    #[must_use]
    pub fn render(&self) -> String {
        let mut lines = vec![format!("Turtle-Job: {}", self.job)];
        for (k, v) in [
            ("Turtle-Task", &self.task),
            ("Turtle-Run", &self.run),
            ("Turtle-Provider", &self.provider),
            ("Turtle-Model", &self.model),
            ("Turtle-Context", &self.context),
        ] {
            if let Some(v) = v {
                lines.push(format!("{k}: {v}"));
            }
        }
        lines.join("\n")
    }
}

fn message(subject: &str, body: &str, trailers: &Trailers) -> Result<String> {
    if subject.is_empty() || subject.contains('\n') {
        return Err(GitError::Invalid(
            "commit subject must be one non-empty line".into(),
        ));
    }
    let mut m = subject.to_owned();
    if !body.trim().is_empty() {
        m.push_str("\n\n");
        m.push_str(body.trim_end());
    }
    m.push_str("\n\n");
    m.push_str(&trailers.render());
    m.push('\n');
    Ok(m)
}

/// A symlink whose target leaves the work tree is refused.
fn symlink_escapes(link_dir: &str, target: &str) -> bool {
    if target.starts_with('/') {
        return true;
    }
    let mut depth = i32::try_from(Path::new(link_dir).components().count()).unwrap_or(0);
    for c in Path::new(target).components() {
        match c {
            Component::ParentDir => depth -= 1,
            Component::Normal(_) => depth += 1,
            _ => {}
        }
        if depth < 0 {
            return true;
        }
    }
    false
}

impl Repo {
    /// Stages everything, checks the staged content against commit policy, and commits with
    /// trailers and without hooks. Returns `None` when there is nothing to commit.
    pub async fn commit_all(
        &self,
        subject: &str,
        body: &str,
        trailers: &Trailers,
    ) -> Result<Option<String>> {
        let msg = message(subject, body, trailers)?;
        self.run(&["add", "-A"]).await?;
        let staged = self.run(&["diff", "--cached", "--name-only", "-z"]).await?;
        let paths: Vec<&str> = staged.split('\0').filter(|p| !p.is_empty()).collect();
        if paths.is_empty() {
            return Ok(None);
        }

        let forbidden: Vec<&str> = paths
            .iter()
            .copied()
            .filter(|p| NeverInclude::matches(p))
            .collect();
        if !forbidden.is_empty() {
            self.run(&["reset", "-q"]).await?;
            return Err(GitError::CommitRefused(format!(
                "staged files match the never-include list: {}",
                forbidden.join(", ")
            )));
        }
        // Symlinks (mode 120000) must stay inside the work tree; large files need approval.
        let raw = self
            .run(&["diff", "--cached", "--raw", "-z", "--no-renames"])
            .await?;
        let mut fields = raw.split('\0').filter(|f| !f.is_empty());
        while let (Some(meta), Some(path)) = (fields.next(), fields.next()) {
            // `:<old mode> <new mode> <old sha> <new sha> <status>`
            if meta.split(' ').nth(1) == Some("120000") {
                let target = std::fs::read_link(self.path().join(path))
                    .map(|t| t.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let dir = path.rsplit_once('/').map_or("", |(d, _)| d);
                if symlink_escapes(dir, &target) {
                    self.run(&["reset", "-q"]).await?;
                    return Err(GitError::CommitRefused(format!(
                        "symlink `{path}` points outside the work tree"
                    )));
                }
            }
            if let Ok(meta) = std::fs::metadata(self.path().join(path))
                && meta.len() > MAX_FILE_BYTES
            {
                self.run(&["reset", "-q"]).await?;
                return Err(GitError::CommitRefused(format!(
                    "`{path}` is larger than {MAX_FILE_BYTES} bytes"
                )));
            }
        }

        self.run_stdin(
            &[
                "-c",
                "commit.gpgsign=false",
                "commit",
                "--no-verify",
                "-q",
                "-F",
                "-",
            ],
            &msg,
        )
        .await?;
        Ok(Some(self.head().await?))
    }

    /// Finds the commit for a plan task via its `Turtle-Task` trailer (recovery probe).
    pub async fn find_commit_by_task(
        &self,
        branch: &str,
        task_key: &str,
    ) -> Result<Option<String>> {
        let pattern = format!("^Turtle-Task: {task_key}$");
        let out = self
            .run(&[
                "log",
                "-n",
                "1",
                "--format=%H",
                "-E",
                "--grep",
                &pattern,
                branch,
            ])
            .await?;
        Ok((!out.is_empty()).then_some(out))
    }

    /// The tree object of a revision (what gate results are bound to).
    pub async fn tree_of(&self, rev: &str) -> Result<String> {
        self.run(&["rev-parse", "--verify", &format!("{rev}^{{tree}}")])
            .await
    }

    /// Finds the commit recorded for a run via its trailer (recovery probe, boundary 16).
    pub async fn find_commit_by_run(&self, branch: &str, run_id: &str) -> Result<Option<String>> {
        let pattern = format!("^Turtle-Run: {run_id}$");
        let out = self
            .run(&[
                "log",
                "-n",
                "1",
                "--format=%H",
                "-E",
                "--grep",
                &pattern,
                branch,
            ])
            .await?;
        Ok((!out.is_empty()).then_some(out))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn never_include_matches_secrets() {
        for p in [
            ".env",
            "config/.env.local",
            "a/b/server.pem",
            "id_rsa",
            "x/secrets/token.txt",
            "terraform.tfstate",
            ".npmrc",
            ".git/config",
        ] {
            assert!(NeverInclude::matches(p), "{p}");
        }
        for p in [
            "src/main.rs",
            "docs/environment.md",
            "idea.txt",
            "keyboard.rs",
        ] {
            assert!(!NeverInclude::matches(p), "{p}");
        }
    }

    #[test]
    fn trailers_render_in_order() {
        let t = Trailers {
            job: "API-42".into(),
            task: Some("T1".into()),
            run: Some("R1".into()),
            provider: Some("codex".into()),
            model: None,
            context: None,
        };
        assert_eq!(
            t.render(),
            "Turtle-Job: API-42\nTurtle-Task: T1\nTurtle-Run: R1\nTurtle-Provider: codex"
        );
    }

    #[test]
    fn symlink_escape_detection() {
        assert!(symlink_escapes("", "/etc/passwd"));
        assert!(symlink_escapes("", "../outside"));
        assert!(symlink_escapes("a", "../../outside"));
        assert!(!symlink_escapes("a/b", "../c"));
        assert!(!symlink_escapes("", "inside/file"));
    }

    #[test]
    fn message_requires_single_line_subject() {
        let t = Trailers {
            job: "API-1".into(),
            ..Trailers::default()
        };
        assert!(message("a\nb", "", &t).is_err());
        assert!(message("", "", &t).is_err());
        assert!(
            message("feat(API-1): x", "body", &t)
                .expect("ok")
                .ends_with("Turtle-Job: API-1\n")
        );
    }
}
