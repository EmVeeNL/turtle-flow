//! Deterministic checks over the committed implementation diff.

use serde_json::{Value, json};
use turtle_domain::paths::{DEFAULT_PROTECTED_PATHS, flags_for_changes, within_areas};
use turtle_domain::{Specification, StaticGateSettings};
use turtle_git::diff::ChangedFile;

#[derive(Debug, Clone)]
pub struct StaticGateReport {
    pub passed: bool,
    pub results: Vec<Value>,
    pub acknowledgement_flags: Vec<String>,
}

/// Evaluate configured static checks without retaining diff contents in the result.
#[must_use]
pub fn evaluate(
    settings: &StaticGateSettings,
    protected_paths: Option<&[String]>,
    spec: &Specification,
    changed: &[ChangedFile],
    diff: &[u8],
) -> StaticGateReport {
    let paths: Vec<String> = changed
        .iter()
        .flat_map(|file| std::iter::once(file.path.clone()).chain(file.old_path.clone()))
        .collect();
    let protected = protected_paths.map_or_else(
        || {
            DEFAULT_PROTECTED_PATHS
                .iter()
                .map(ToString::to_string)
                .collect()
        },
        <[String]>::to_vec,
    );
    let mut flags = flags_for_changes(&paths, &protected);
    if !settings.protected_paths {
        flags.retain(|flag| !flag.starts_with("protected_path:"));
    }
    if !settings.dependency_changes {
        flags.retain(|flag| !flag.starts_with("dependency_manifest:"));
    }
    let text = String::from_utf8_lossy(diff);
    let (_, secret_count) = turtle_context::redact(&text);
    let added = changed.iter().filter_map(|file| file.added).sum::<u64>();
    let removed = changed.iter().filter_map(|file| file.removed).sum::<u64>();
    let scope_violations: Vec<String> = paths
        .iter()
        .filter(|path| !within_areas(path, &spec.affected_areas))
        .cloned()
        .collect();
    let mut results = Vec::new();
    let mut passed = true;
    if settings.secret_scan {
        let ok = secret_count == 0;
        passed &= ok;
        results.push(json!({"gate":"secret_scan", "outcome": if ok {"passed"} else {"failed"},
            "summary": if ok {"no supported credential patterns found"} else {"credential pattern detected; diff content was discarded"}}));
    }
    if settings.protected_paths {
        results.push(json!({"gate":"protected_paths", "outcome":"passed", "flags": flags.iter().filter(|flag| flag.starts_with("protected_path:")).collect::<Vec<_>>() }));
    }
    if let Some(limit) = settings.max_diff_lines {
        let changed_lines = added.saturating_add(removed);
        let ok = changed_lines <= limit;
        passed &= ok;
        results.push(json!({"gate":"diff_size", "outcome": if ok {"passed"} else {"failed"}, "changed_lines":changed_lines, "limit":limit}));
    }
    if settings.scope_conformance {
        let ok = scope_violations.is_empty();
        passed &= ok;
        results.push(json!({"gate":"scope_conformance", "outcome": if ok {"passed"} else {"failed"}, "violations":scope_violations}));
    }
    if settings.dependency_changes {
        results.push(json!({"gate":"dependency_changes", "outcome":"passed", "flags": flags.iter().filter(|flag| flag.starts_with("dependency_manifest:") || flag.starts_with("protected_path:")).collect::<Vec<_>>() }));
    }
    StaticGateReport {
        passed,
        results,
        acknowledgement_flags: flags,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use turtle_domain::Specification;
    use turtle_git::Repo;

    #[tokio::test]
    async fn real_git_diff_flags_secrets_protected_paths_scope_and_size() {
        let dir = tempfile::tempdir().expect("temporary repository");
        let git = |args: &[&str]| {
            let out = Command::new("git")
                .arg("-C")
                .arg(dir.path())
                .args(args)
                .output()
                .expect("git");
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
        };
        git(&["init", "-q"]);
        git(&["config", "user.email", "test@example.invalid"]);
        git(&["config", "user.name", "Test"]);
        std::fs::write(dir.path().join("src.rs"), "old\n").expect("seed file");
        git(&["add", "src.rs"]);
        git(&["commit", "-qm", "base"]);
        std::fs::write(
            dir.path().join("src.rs"),
            "api_key=ghp_abcdefghijklmnopqrstuvwxyz\nextra\n",
        )
        .expect("change source");
        std::fs::write(dir.path().join("Cargo.lock"), "version = 4\n").expect("lockfile");
        git(&["add", "src.rs", "Cargo.lock"]);
        git(&["commit", "-qm", "change"]);
        let repo = Repo::new(dir.path());
        let changed = repo
            .changed_files("HEAD~1", "HEAD")
            .await
            .expect("changed files");
        let diff = repo.diff_bytes("HEAD~1", "HEAD").await.expect("diff");
        let spec = Specification {
            summary: "test scope".into(),
            in_scope: vec![],
            out_of_scope: vec![],
            criteria: vec![],
            affected_areas: vec!["docs/**".into()],
            risks: vec![],
            assumptions: vec![],
        };
        let report = evaluate(
            &StaticGateSettings {
                max_diff_lines: Some(1),
                ..StaticGateSettings::default()
            },
            None,
            &spec,
            &changed,
            &diff,
        );
        assert!(!report.passed);
        assert!(
            report
                .results
                .iter()
                .any(|r| r["gate"] == "secret_scan" && r["outcome"] == "failed")
        );
        assert!(
            report
                .results
                .iter()
                .any(|r| r["gate"] == "scope_conformance" && r["outcome"] == "failed")
        );
        assert!(
            report
                .results
                .iter()
                .any(|r| r["gate"] == "diff_size" && r["outcome"] == "failed")
        );
        assert!(
            report
                .acknowledgement_flags
                .iter()
                .any(|flag| flag == "protected_path:Cargo.lock")
        );
        let serialized = serde_json::to_string(&report.results).expect("serialize report");
        assert!(!serialized.contains("ghp_abcdefghijklmnopqrstuvwxyz"));
    }
}
