//! `turtle doctor`: environment checks (FR-135). Read-only: never creates files, and reads
//! capability snapshots without migrating or writing the database.

use std::path::PathBuf;
use std::process::ExitCode;

use serde::Serialize;
use serde_json::Value;
use turtle_agents::{CapabilityDiff, CompatStatus, ProviderId, ProviderInfo};
use turtle_store::CapabilitySnapshot;

use crate::capabilities::short_hash;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Level {
    Pass,
    Warn,
    Fail,
}

#[derive(Debug, Serialize)]
struct Check {
    name: String,
    level: Level,
    detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    fix: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    snapshot: Option<SnapshotReport>,
}

/// The current capability snapshot and its difference from the previous one. When the live
/// discovery differs from the latest stored snapshot, `current` is the unrecorded live set and
/// `previous` the latest stored one.
#[derive(Debug, Serialize)]
struct SnapshotReport {
    current: SnapshotView,
    #[serde(skip_serializing_if = "Option::is_none")]
    previous: Option<SnapshotView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    diff: Option<CapabilityDiff>,
}

#[derive(Debug, Serialize)]
struct SnapshotView {
    /// Per-provider snapshot number; `None` for a live set not recorded yet.
    version: Option<i64>,
    cli_version: Option<String>,
    content_hash: String,
    discovered_at: Option<String>,
    capabilities: Value,
}

impl SnapshotView {
    fn stored(s: &CapabilitySnapshot) -> Self {
        Self {
            version: Some(s.version),
            cli_version: s.cli_version.clone(),
            content_hash: s.content_hash.clone(),
            discovered_at: Some(s.discovered_at.clone()),
            capabilities: s.capabilities.clone(),
        }
    }

    fn diff_from(&self, previous: &Self) -> CapabilityDiff {
        CapabilityDiff::between(
            previous.cli_version.as_deref(),
            &previous.capabilities,
            self.cli_version.as_deref(),
            &self.capabilities,
        )
    }
}

pub async fn run(json: bool) -> ExitCode {
    let mut checks = vec![check_git().await];
    for provider in ProviderId::ALL {
        let info = turtle_agents::discover(provider).await;
        checks.push(check_provider(provider, &info));
        checks.push(check_capabilities(provider, &info));
    }
    // At least one provider must be usable.
    let usable = checks
        .iter()
        .filter(|c| c.name.starts_with("provider:") && c.level != Level::Fail)
        .count();
    if usable == 0 {
        checks.push(Check {
            name: "providers".into(),
            level: Level::Fail,
            detail: "no supported AI agent CLI is installed".into(),
            snapshot: None,
            fix: Some("install Claude Code or Codex CLI and log in".into()),
        });
    }
    checks.push(check_data_dir());
    checks.push(check_approval_key());

    if json {
        let out = serde_json::to_string_pretty(&checks)
            .unwrap_or_else(|e| format!("{{\"error\":\"{e}\"}}"));
        println!("{out}");
    } else {
        for c in &checks {
            let tag = match c.level {
                Level::Pass => "pass",
                Level::Warn => "warn",
                Level::Fail => "FAIL",
            };
            println!("{tag:>4}  {:<24} {}", c.name, c.detail);
            if let Some(fix) = &c.fix {
                println!("      {:<24} → {fix}", "");
            }
        }
    }

    if checks.iter().any(|c| c.level == Level::Fail) {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

async fn check_git() -> Check {
    match turtle_git::version().await {
        Ok(v) if v >= turtle_git::MIN_GIT_VERSION => Check {
            name: "git".into(),
            level: Level::Pass,
            detail: format!("git {v}"),
            snapshot: None,
            fix: None,
        },
        Ok(v) => Check {
            name: "git".into(),
            level: Level::Fail,
            detail: format!("git {v} is older than {}", turtle_git::MIN_GIT_VERSION),
            snapshot: None,
            fix: Some("upgrade Git (merge-tree --write-tree is required)".into()),
        },
        Err(e) => Check {
            name: "git".into(),
            level: Level::Fail,
            detail: e.to_string(),
            snapshot: None,
            fix: Some("install Git ≥ 2.38".into()),
        },
    }
}

fn check_provider(provider: ProviderId, info: &ProviderInfo) -> Check {
    let version = info
        .version
        .as_ref()
        .map_or_else(String::new, |v| format!("{v} — "));
    let (level, fix) = match info.compat {
        CompatStatus::Supported => (Level::Pass, None),
        CompatStatus::Untested => (Level::Warn, None),
        CompatStatus::NotInstalled => (
            Level::Warn,
            Some(format!("optional: install `{}`", provider.binary())),
        ),
        CompatStatus::Broken => (
            Level::Fail,
            Some(format!("check `{} --version`", provider.binary())),
        ),
    };
    Check {
        name: format!("provider:{}", provider.as_str()),
        level,
        detail: format!("{version}{}", info.detail),
        fix,
        snapshot: None,
    }
}

/// Compares the live capability set with the stored snapshots (issue #61).
fn check_capabilities(provider: ProviderId, info: &ProviderInfo) -> Check {
    let name = format!("capabilities:{}", provider.as_str());
    let check = |level, detail: String, fix: Option<String>, snapshot| Check {
        name: name.clone(),
        level,
        detail,
        fix,
        snapshot,
    };
    let stored = match data_dir()
        .map(|dir| {
            turtle_store::read_capability_snapshots(&dir.join("turtle.db"), provider.as_str(), 2)
        })
        .transpose()
    {
        Ok(stored) => stored.flatten().unwrap_or_default(),
        Err(e) => {
            return check(
                Level::Warn,
                format!("could not read capability snapshots: {e}"),
                None,
                None,
            );
        }
    };
    let live = info.version.as_ref().map(|version| {
        let cli_version = version.to_string();
        let capabilities = turtle_agents::verified_capabilities(provider, info.compat).to_json();
        SnapshotView {
            version: None,
            content_hash: turtle_store::capability_hash(
                provider.as_str(),
                Some(&cli_version),
                &capabilities,
            ),
            cli_version: Some(cli_version),
            discovered_at: None,
            capabilities,
        }
    });
    let latest = stored.first().map(SnapshotView::stored);
    let older = stored.get(1).map(SnapshotView::stored);
    let refresh = "run `turtle providers refresh` (or start the daemon) to record it";
    match (live, latest) {
        (None, None) => check(Level::Pass, "no snapshot recorded".into(), None, None),
        (None, Some(latest)) => check(
            Level::Pass,
            format!(
                "provider unavailable; last snapshot #{} (cli {})",
                latest.version.unwrap_or_default(),
                latest.cli_version.as_deref().unwrap_or("unknown")
            ),
            None,
            Some(SnapshotReport {
                current: latest,
                previous: older,
                diff: None,
            }),
        ),
        (Some(live), None) => check(
            Level::Pass,
            format!(
                "not recorded yet (cli {}, {})",
                live.cli_version.as_deref().unwrap_or("unknown"),
                short_hash(&live.content_hash)
            ),
            Some(refresh.into()),
            Some(SnapshotReport {
                current: live,
                previous: None,
                diff: None,
            }),
        ),
        (Some(live), Some(latest)) if live.content_hash == latest.content_hash => {
            let diff = older.as_ref().map(|older| latest.diff_from(older));
            let detail = format!(
                "snapshot #{} (cli {}, {}); {}",
                latest.version.unwrap_or_default(),
                latest.cli_version.as_deref().unwrap_or("unknown"),
                short_hash(&latest.content_hash),
                match (&older, &diff) {
                    (Some(older), Some(diff)) => format!(
                        "since #{}: {}",
                        older.version.unwrap_or_default(),
                        diff.summary()
                    ),
                    _ => "first snapshot".into(),
                }
            );
            check(
                Level::Pass,
                detail,
                None,
                Some(SnapshotReport {
                    current: latest,
                    previous: older,
                    diff,
                }),
            )
        }
        (Some(live), Some(latest)) => {
            let diff = live.diff_from(&latest);
            check(
                Level::Warn,
                format!(
                    "changed since snapshot #{} (not recorded): {}",
                    latest.version.unwrap_or_default(),
                    diff.summary()
                ),
                Some(refresh.into()),
                Some(SnapshotReport {
                    current: live,
                    previous: Some(latest),
                    diff: Some(diff),
                }),
            )
        }
    }
}

fn data_dir() -> Option<PathBuf> {
    std::env::var_os("TURTLE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".turtle")))
}

fn check_data_dir() -> Check {
    let Some(dir) = data_dir() else {
        return Check {
            name: "data-dir".into(),
            level: Level::Fail,
            detail: "neither TURTLE_HOME nor HOME is set".into(),
            snapshot: None,
            fix: Some("set TURTLE_HOME".into()),
        };
    };
    match std::fs::metadata(&dir) {
        Err(_) => Check {
            name: "data-dir".into(),
            level: Level::Pass,
            detail: format!("{} (not created yet)", dir.display()),
            snapshot: None,
            fix: None,
        },
        Ok(meta) => {
            use std::os::unix::fs::PermissionsExt;
            let mode = meta.permissions().mode() & 0o777;
            if mode & 0o077 == 0 {
                Check {
                    name: "data-dir".into(),
                    level: Level::Pass,
                    detail: format!("{} (mode {mode:o})", dir.display()),
                    snapshot: None,
                    fix: None,
                }
            } else {
                Check {
                    name: "data-dir".into(),
                    level: Level::Fail,
                    detail: format!(
                        "{} is accessible by other users (mode {mode:o})",
                        dir.display()
                    ),
                    snapshot: None,
                    fix: Some(format!("chmod 700 {}", dir.display())),
                }
            }
        }
    }
}

fn check_approval_key() -> Check {
    let Some(dir) = data_dir() else {
        return Check {
            name: "approval-key".into(),
            level: Level::Fail,
            detail: "data directory unavailable".into(),
            snapshot: None,
            fix: None,
        };
    };
    let key = dir.join("turtle.approval-key");
    if key.exists() {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&key).map(|m| m.permissions().mode() & 0o777);
        if !mode.is_ok_and(|m| m & 0o077 == 0) {
            return Check {
                name: "approval-key".into(),
                level: Level::Fail,
                detail: "fallback key file is readable by other users".into(),
                snapshot: None,
                fix: Some(format!("chmod 600 {}", key.display())),
            };
        }
        Check {
            name: "approval-key".into(),
            level: Level::Warn,
            detail: "0600 file fallback (weaker than OS keychain)".into(),
            snapshot: None,
            fix: Some(
                "configure macOS Keychain or Linux Secret Service for stronger key isolation"
                    .into(),
            ),
        }
    } else {
        Check {
            name: "approval-key".into(),
            level: Level::Warn,
            detail: "no fallback file; keychain status unknown without opening the database".into(),
            snapshot: None,
            fix: None,
        }
    }
}
