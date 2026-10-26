//! `turtle doctor`: environment checks (FR-135). Read-only: never creates files.

use std::path::PathBuf;
use std::process::ExitCode;

use serde::Serialize;
use turtle_agents::{CompatStatus, ProviderId};

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
}

pub async fn run(json: bool) -> ExitCode {
    let mut checks = vec![check_git().await];
    for provider in ProviderId::ALL {
        checks.push(check_provider(provider).await);
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
            println!("{tag:>4}  {:<20} {}", c.name, c.detail);
            if let Some(fix) = &c.fix {
                println!("      {:<20} → {fix}", "");
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
            fix: None,
        },
        Ok(v) => Check {
            name: "git".into(),
            level: Level::Fail,
            detail: format!("git {v} is older than {}", turtle_git::MIN_GIT_VERSION),
            fix: Some("upgrade Git (merge-tree --write-tree is required)".into()),
        },
        Err(e) => Check {
            name: "git".into(),
            level: Level::Fail,
            detail: e.to_string(),
            fix: Some("install Git ≥ 2.38".into()),
        },
    }
}

async fn check_provider(provider: ProviderId) -> Check {
    let info = turtle_agents::discover(provider).await;
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
            fix: Some("set TURTLE_HOME".into()),
        };
    };
    match std::fs::metadata(&dir) {
        Err(_) => Check {
            name: "data-dir".into(),
            level: Level::Pass,
            detail: format!("{} (not created yet)", dir.display()),
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
                fix: Some(format!("chmod 600 {}", key.display())),
            };
        }
        Check {
            name: "approval-key".into(),
            level: Level::Warn,
            detail: "0600 file fallback (weaker than OS keychain)".into(),
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
            fix: None,
        }
    }
}
