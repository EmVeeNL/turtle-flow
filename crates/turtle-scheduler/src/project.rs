//! Project registration, including the repository checks used by `turtle project add`.

use std::path::{Path, PathBuf};

use thiserror::Error;
use turtle_domain::{JobKey, ProjectCommands, ProjectId, ProjectSettings};
use turtle_git::Repo;
use turtle_store::{NewProject, StoreError};

use crate::SharedStore;

#[derive(Debug, Clone)]
pub struct AddProject {
    pub path: PathBuf,
    pub slug: Option<String>,
    pub name: Option<String>,
    pub key: Option<String>,
    pub profile: String,
    pub branch: Option<String>,
}

#[derive(Debug, Error)]
pub enum ProjectError {
    #[error("{0}")]
    Invalid(String),
    #[error(transparent)]
    Store(#[from] StoreError),
}

#[derive(Debug, Clone)]
pub struct ProjectService {
    store: SharedStore,
}

impl ProjectService {
    pub fn new(store: SharedStore) -> Self {
        Self { store }
    }

    pub async fn add(&self, input: AddProject) -> Result<(ProjectId, String), ProjectError> {
        if !["fast", "standard", "strict"].contains(&input.profile.as_str()) {
            return Err(ProjectError::Invalid(
                "profile must be fast, standard, or strict".into(),
            ));
        }
        let path = input.path.canonicalize().map_err(|_| {
            ProjectError::Invalid(format!("{} does not exist", input.path.display()))
        })?;
        let repo = Repo::new(&path);
        if repo.run(&["rev-parse", "--git-dir"]).await.is_err() {
            return Err(ProjectError::Invalid(format!(
                "{} is not a Git repository",
                path.display()
            )));
        }
        if repo
            .run(&["rev-parse", "--is-bare-repository"])
            .await
            .map_err(|_| ProjectError::Invalid("cannot inspect repository".into()))?
            == "true"
        {
            return Err(ProjectError::Invalid(
                "repository is bare; point at a working checkout".into(),
            ));
        }
        let toplevel = repo
            .run(&["rev-parse", "--show-toplevel"])
            .await
            .map_err(|_| ProjectError::Invalid("cannot inspect repository".into()))?;
        if repo
            .run(&["rev-parse", "--verify", "-q", "HEAD"])
            .await
            .is_err()
        {
            return Err(ProjectError::Invalid(format!(
                "{toplevel} has no commits yet; make an initial commit first"
            )));
        }
        if repo
            .run(&["rev-parse", "--verify", "-q", "MERGE_HEAD"])
            .await
            .is_ok()
        {
            return Err(ProjectError::Invalid(format!(
                "{toplevel} has an unresolved merge; finish or abort it first"
            )));
        }
        let branch = match input.branch {
            Some(branch) => branch,
            None => repo
                .run(&["symbolic-ref", "--short", "-q", "HEAD"])
                .await
                .map_err(|_| {
                    ProjectError::Invalid(format!("{toplevel} has a detached HEAD; pass --branch"))
                })?,
        };
        let dir_name = Path::new(&toplevel).file_name().map_or_else(
            || "project".to_owned(),
            |name| name.to_string_lossy().into_owned(),
        );
        let slug = match input.slug {
            Some(slug) => slug,
            None => slugify(&dir_name).ok_or_else(|| {
                ProjectError::Invalid(
                    "cannot derive a project slug from repository name; pass slug".into(),
                )
            })?,
        };
        let key = match input.key {
            Some(key) => key,
            None => derive_key(&slug).ok_or_else(|| {
                ProjectError::Invalid(format!(
                    "cannot derive a job key prefix from `{slug}`; pass --key (2-6 uppercase letters)"
                ))
            })?,
        };
        JobKey::new(key.clone(), 1)
            .map_err(|error| ProjectError::Invalid(format!("key: {error}")))?;

        let project = NewProject {
            name: input.name.unwrap_or_else(|| slug.clone()),
            slug: slug.clone(),
            key_prefix: key,
            description: String::new(),
            repo_path: toplevel,
            default_branch: branch.clone(),
            default_profile: input.profile,
        };
        let settings = ProjectSettings {
            commands: ProjectCommands::default(),
            target_branch: Some(branch),
            ..ProjectSettings::default()
        };
        let id = self
            .store
            .with(move |store| {
                store.transaction(|tx| {
                    let id = tx.insert_project(&project)?;
                    tx.update_project_settings(id, &settings)?;
                    Ok(id)
                })
            })
            .await?;
        Ok((id, slug))
    }
}

fn slugify(text: &str) -> Option<String> {
    let mut slug = String::new();
    for character in text.chars() {
        if character.is_ascii_alphanumeric() {
            slug.push(character.to_ascii_lowercase());
        } else if !slug.ends_with('-') && !slug.is_empty() {
            slug.push('-');
        }
    }
    let slug = slug.trim_end_matches('-').to_owned();
    (!slug.is_empty()).then_some(slug)
}

fn derive_key(slug: &str) -> Option<String> {
    let key: String = slug
        .chars()
        .filter(char::is_ascii_alphabetic)
        .take(3)
        .collect::<String>()
        .to_ascii_uppercase();
    (key.len() >= 2).then_some(key)
}
