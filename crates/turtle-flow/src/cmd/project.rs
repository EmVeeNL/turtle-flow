//! `turtle project add | list | show`.

use std::path::Path;

use anyhow::{Context, Result, bail};
use clap::Subcommand;
use turtle_domain::{DeliveryMode, ProjectCommands, ProjectSettings};
use turtle_git::Repo;
use turtle_store::NewProject;

use crate::runtime::Runtime;

#[derive(Debug, Subcommand)]
#[allow(clippy::large_enum_variant)] // parsed once per process; boxing buys nothing
pub enum ProjectCommand {
    /// Register an existing Git repository.
    Add {
        path: std::path::PathBuf,
        /// Short name used on the command line (default: derived from the directory).
        #[arg(long)]
        slug: Option<String>,
        /// Display name (default: the slug).
        #[arg(long)]
        name: Option<String>,
        /// 2-6 uppercase letters used in job keys such as `API-42` (default: derived from the slug).
        #[arg(long)]
        key: Option<String>,
        /// Default workflow profile for new jobs.
        #[arg(long, default_value = "standard", value_parser = ["fast", "standard", "strict"])]
        profile: String,
        /// Branch that deliveries go to (default: the branch currently checked out).
        #[arg(long)]
        branch: Option<String>,
        /// Gate command: build the project.
        #[arg(long)]
        build: Option<String>,
        /// Gate command: run the tests.
        #[arg(long)]
        test: Option<String>,
        /// Gate command: lint.
        #[arg(long)]
        lint: Option<String>,
        /// Command run once after a job worktree is created (install dependencies).
        #[arg(long)]
        worktree_setup: Option<String>,
        /// Delivery mode: local-squash (default) or pr.
        #[arg(long, default_value = "local-squash", value_parser = ["local-squash", "pr"])]
        delivery: String,
    },
    /// Change how future deliveries for this project are made.
    Delivery {
        slug: String,
        #[arg(value_parser = ["local-squash", "pr"])]
        mode: String,
    },
    /// List registered projects.
    List,
    /// Show one project.
    Show { slug: String },
}

pub async fn run(rt: &Runtime, command: ProjectCommand) -> Result<()> {
    match command {
        ProjectCommand::Add {
            path,
            slug,
            name,
            key,
            profile,
            branch,
            build,
            test,
            lint,
            worktree_setup,
            delivery,
        } => {
            let repo = inspect_repo(&path).await?;
            let branch = match branch {
                Some(b) => b,
                None => repo.branch,
            };
            let slug = match slug {
                Some(s) => s,
                None => slugify(&repo.dir_name)?,
            };
            let key_prefix = match key {
                Some(k) => k,
                None => derive_key(&slug)?,
            };
            turtle_domain::JobKey::new(key_prefix.clone(), 1)
                .map_err(|e| anyhow::anyhow!("--key: {e}"))?;
            let settings = ProjectSettings {
                commands: ProjectCommands {
                    build,
                    test,
                    lint,
                    worktree_setup,
                    ..ProjectCommands::default()
                },
                target_branch: Some(branch.clone()),
                delivery: parse_delivery(&delivery)?,
                ..ProjectSettings::default()
            };
            let new = NewProject {
                name: name.unwrap_or_else(|| slug.clone()),
                slug: slug.clone(),
                key_prefix: key_prefix.clone(),
                description: String::new(),
                repo_path: repo.toplevel,
                default_branch: branch.clone(),
                default_profile: profile,
            };
            let created = rt
                .store
                .with(move |s| {
                    s.transaction(|tx| {
                        let id = tx.insert_project(&new)?;
                        tx.update_project_settings(id, &settings)?;
                        Ok(id)
                    })
                })
                .await
                .map_err(|e| match e {
                    turtle_store::StoreError::Sqlite(rusqlite::Error::SqliteFailure(f, _))
                        if f.code == rusqlite::ErrorCode::ConstraintViolation =>
                    {
                        anyhow::anyhow!(
                            "a project with slug `{slug}` or key `{key_prefix}` already exists"
                        )
                    }
                    other => other.into(),
                })?;
            let _ = created;
            println!(
                "Registered {slug} (jobs will be {key_prefix}-1, {key_prefix}-2, ...) targeting `{branch}`."
            );
            println!("Next: turtle job create -p {slug} \"what you want built\" --start");
            Ok(())
        }
        ProjectCommand::Delivery { slug, mode } => {
            let project = rt.store.with(move |s| s.project_by_slug(&slug)).await?;
            let mut settings = project.settings;
            settings.delivery = parse_delivery(&mode)?;
            let id = project.id;
            rt.store
                .with(move |s| s.transaction(|tx| tx.update_project_settings(id, &settings)))
                .await?;
            println!("Delivery mode set to {mode}.");
            Ok(())
        }
        ProjectCommand::List => {
            let projects = rt.store.with(|s| s.list_projects()).await?;
            if projects.is_empty() {
                println!("No projects yet. Register one with: turtle project add <path>");
                return Ok(());
            }
            for p in projects {
                println!(
                    "{:<20} {:<6} {:<10} {}",
                    p.slug, p.key_prefix, p.default_branch, p.repo_path
                );
            }
            Ok(())
        }
        ProjectCommand::Show { slug } => {
            let p = rt.store.with(move |s| s.project_by_slug(&slug)).await?;
            println!("{} ({})", p.name, p.slug);
            println!("  key prefix   {}", p.key_prefix);
            println!("  repository   {}", p.repo_path);
            println!(
                "  delivery     {}",
                match p.settings.delivery {
                    DeliveryMode::LocalSquash => "local-squash",
                    DeliveryMode::Pr => "pr",
                }
            );
            println!(
                "  target       {}",
                p.settings
                    .target_branch
                    .as_deref()
                    .unwrap_or(&p.default_branch)
            );
            let gates = p.settings.commands.gates();
            if gates.is_empty() {
                println!("  gates        none configured (jobs will not run build or tests)");
            }
            for (gate, command) in gates {
                println!("  gate {gate:<7} {command}");
            }
            Ok(())
        }
    }
}

fn parse_delivery(value: &str) -> Result<DeliveryMode> {
    match value {
        "local-squash" => Ok(DeliveryMode::LocalSquash),
        "pr" => Ok(DeliveryMode::Pr),
        _ => bail!("delivery must be local-squash or pr"),
    }
}

struct RepoInfo {
    toplevel: String,
    branch: String,
    dir_name: String,
}

/// Accepts only a normal repository with at least one commit and no merge in progress (FR-001).
async fn inspect_repo(path: &Path) -> Result<RepoInfo> {
    let path = path
        .canonicalize()
        .with_context(|| format!("{} does not exist", path.display()))?;
    let repo = Repo::new(&path);
    if repo.run(&["rev-parse", "--git-dir"]).await.is_err() {
        bail!("{} is not a Git repository", path.display());
    }
    if repo.run(&["rev-parse", "--is-bare-repository"]).await? == "true" {
        bail!(
            "{} is a bare repository; point at a working checkout",
            path.display()
        );
    }
    let toplevel = repo.run(&["rev-parse", "--show-toplevel"]).await?;
    if repo
        .run(&["rev-parse", "--verify", "-q", "HEAD"])
        .await
        .is_err()
    {
        bail!("{toplevel} has no commits yet; make an initial commit first");
    }
    if repo
        .run(&["rev-parse", "--verify", "-q", "MERGE_HEAD"])
        .await
        .is_ok()
    {
        bail!("{toplevel} has an unresolved merge; finish or abort it first");
    }
    let branch = repo
        .run(&["symbolic-ref", "--short", "-q", "HEAD"])
        .await
        .map_err(|_| anyhow::anyhow!("{toplevel} has a detached HEAD; pass --branch"))?;
    let dir_name = Path::new(&toplevel).file_name().map_or_else(
        || "project".to_owned(),
        |n| n.to_string_lossy().into_owned(),
    );
    Ok(RepoInfo {
        toplevel,
        branch,
        dir_name,
    })
}

fn slugify(text: &str) -> Result<String> {
    let mut slug = String::new();
    for c in text.chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
        } else if !slug.ends_with('-') && !slug.is_empty() {
            slug.push('-');
        }
    }
    let slug = slug.trim_end_matches('-').to_owned();
    if slug.is_empty() {
        bail!("cannot derive a project slug from `{text}`; pass --slug");
    }
    Ok(slug)
}

/// `payments-api` becomes `PAY`: the first three letters, upper-cased.
fn derive_key(slug: &str) -> Result<String> {
    let key: String = slug
        .chars()
        .filter(char::is_ascii_alphabetic)
        .take(3)
        .collect::<String>()
        .to_ascii_uppercase();
    if key.len() < 2 {
        bail!("cannot derive a job key prefix from `{slug}`; pass --key (2-6 uppercase letters)");
    }
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_and_keys_are_derived_from_directory_names() {
        assert_eq!(slugify("Payments API_v2").expect("slug"), "payments-api-v2");
        assert_eq!(derive_key("payments-api").expect("key"), "PAY");
        assert_eq!(derive_key("ab").expect("key"), "AB");
        assert!(derive_key("42").is_err());
        assert!(slugify("---").is_err());
    }
}
