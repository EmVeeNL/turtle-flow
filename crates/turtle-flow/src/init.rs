//! Interactive first-run setup. Detection only proposes gate commands; registration waits for
//! the user's explicit confirmation.

use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::cmd::project::{ProjectCommand, run as run_project};
use crate::paths::Home;
use crate::runtime::Runtime;

#[derive(Debug, Default, PartialEq, Eq)]
struct Suggestions {
    build: Option<String>,
    test: Option<String>,
    lint: Option<String>,
}

fn detect(path: &Path) -> Suggestions {
    if path.join("Cargo.toml").is_file() {
        return Suggestions {
            build: Some("cargo build".into()),
            test: Some("cargo test".into()),
            lint: Some("cargo clippy --all-targets -- -D warnings".into()),
        };
    }
    if path.join("go.mod").is_file() {
        return Suggestions {
            build: Some("go build ./...".into()),
            test: Some("go test ./...".into()),
            lint: [".golangci.yml", ".golangci.yaml"]
                .iter()
                .any(|config| path.join(config).is_file())
                .then(|| "golangci-lint run".into()),
        };
    }
    if path.join("pyproject.toml").is_file() || path.join("pytest.ini").is_file() {
        let pyproject = std::fs::read_to_string(path.join("pyproject.toml")).unwrap_or_default();
        return Suggestions {
            build: pyproject
                .contains("[build-system]")
                .then(|| "python -m build".into()),
            test: Some("python -m pytest".into()),
            lint: (path.join("ruff.toml").is_file() || pyproject.contains("[tool.ruff]"))
                .then(|| "ruff check .".into()),
        };
    }
    if path.join("package.json").is_file() {
        let manager = if path.join("pnpm-lock.yaml").is_file() {
            "pnpm"
        } else {
            "npm"
        };
        let scripts = std::fs::read(path.join("package.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
            .and_then(|json| json.get("scripts").cloned())
            .unwrap_or_default();
        let script = |name: &str| {
            scripts
                .get(name)
                .and_then(serde_json::Value::as_str)
                .map(|_| format!("{manager} run {name}"))
        };
        return Suggestions {
            build: script("build"),
            test: script("test"),
            lint: script("lint"),
        };
    }
    Suggestions::default()
}

fn prompt(label: &str, suggested: Option<&str>) -> Result<Option<String>> {
    let suffix =
        suggested.map_or_else(|| "none detected".to_owned(), |s| format!("suggested: {s}"));
    print!("{label} command ({suffix}); enter to accept, `-` for none, or type a command: ");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let input = input.trim();
    if input == "-" {
        return Ok(None);
    }
    if input.is_empty() {
        return Ok(suggested.map(str::to_owned));
    }
    Ok(Some(input.to_owned()))
}

pub async fn run() -> Result<()> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        bail!(
            "`turtle init` is interactive; run it in a terminal. It will not guess project commands in non-interactive mode."
        );
    }
    println!("Turtle Flow setup checks (read-only):");
    let _ = crate::doctor::run(false).await;
    let cwd = std::env::current_dir().context("reading current directory")?;
    let suggestions = detect(&cwd);
    println!("\nProject: {}", cwd.display());
    println!("Detected gate commands are suggestions. Review each before continuing.");
    let build = prompt("Build", suggestions.build.as_deref())?;
    let test = prompt("Test", suggestions.test.as_deref())?;
    let lint = prompt("Lint", suggestions.lint.as_deref())?;
    print!("Register this repository with `turtle project add .`? [y/N] ");
    io::stdout().flush()?;
    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    if !matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
        println!("No project was registered; no files were written.");
        return Ok(());
    }
    let home = Home::locate()?;
    let rt = Runtime::open(home)?;
    run_project(
        &rt,
        ProjectCommand::Add {
            path: PathBuf::from("."),
            slug: None,
            name: None,
            key: None,
            profile: "standard".into(),
            branch: None,
            build,
            test,
            lint,
            worktree_setup: None,
            delivery: "local-squash".into(),
        },
    )
    .await?;
    println!(
        "\nNext commands:\n  turtle job create \"what you want built\" --start\n  turtle up --agent claude,codex\n  turtle job watch <key>"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn stack_detection_suggests_only_known_commands() {
        let temp = tempdir().expect("temporary directory");
        std::fs::write(temp.path().join("Cargo.toml"), "[package]\n").expect("manifest");
        assert_eq!(
            detect(temp.path()),
            Suggestions {
                build: Some("cargo build".into()),
                test: Some("cargo test".into()),
                lint: Some("cargo clippy --all-targets -- -D warnings".into()),
            }
        );
    }

    #[test]
    fn node_detection_requires_package_scripts_and_prefers_pnpm_lockfile() {
        let temp = tempdir().expect("temporary directory");
        std::fs::write(
            temp.path().join("package.json"),
            r#"{"scripts":{"build":"astro build","test":"vitest run"}}"#,
        )
        .expect("package");
        std::fs::write(
            temp.path().join("pnpm-lock.yaml"),
            "lockfileVersion: '9.0'\n",
        )
        .expect("lock");
        assert_eq!(
            detect(temp.path()),
            Suggestions {
                build: Some("pnpm run build".into()),
                test: Some("pnpm run test".into()),
                lint: None,
            }
        );
    }
}
