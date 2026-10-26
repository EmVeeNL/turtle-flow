//! Per-project settings stored in `projects.settings_json`.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryMode {
    #[default]
    LocalSquash,
    Pr,
}

/// Commands run in the job worktree (FR-004). Each is a shell command line; all are optional.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ProjectCommands {
    pub build: Option<String>,
    pub test: Option<String>,
    pub lint: Option<String>,
    pub format_check: Option<String>,
    /// Runs once after the worktree is created (install dependencies, warm caches).
    pub worktree_setup: Option<String>,
}

impl ProjectCommands {
    /// Gate commands in execution order with their gate names.
    #[must_use]
    pub fn gates(&self) -> Vec<(&'static str, &str)> {
        [
            ("build", &self.build),
            ("test", &self.test),
            ("lint", &self.lint),
            ("format_check", &self.format_check),
        ]
        .into_iter()
        .filter_map(|(name, cmd)| cmd.as_deref().map(|c| (name, c)))
        .collect()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ProjectSettings {
    pub commands: ProjectCommands,
    pub delivery: DeliveryMode,
    /// Branch that deliveries go to; defaults to the project's default branch.
    pub target_branch: Option<String>,
    pub branch_prefix: Option<String>,
    pub gate_timeout_secs: Option<u64>,
    /// Replaces the default protected path list when set.
    pub protected_paths: Option<Vec<String>>,
    /// Extra environment variables passed to gate commands (names only).
    pub env_allowlist: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_json_is_valid_defaults() {
        let s: ProjectSettings = serde_json::from_str("{}").expect("defaults");
        assert_eq!(s, ProjectSettings::default());
        assert!(s.commands.gates().is_empty());
        assert_eq!(s.delivery, DeliveryMode::LocalSquash);
    }

    #[test]
    fn pr_delivery_round_trips_as_project_setting() {
        let settings: ProjectSettings =
            serde_json::from_str(r#"{"delivery":"pr"}"#).expect("PR setting");
        assert_eq!(settings.delivery, DeliveryMode::Pr);
        let encoded = serde_json::to_string(&settings).expect("serialize");
        assert!(encoded.contains("\"delivery\":\"pr\""));
    }

    #[test]
    fn gates_keep_a_fixed_order() {
        let s = ProjectSettings {
            commands: ProjectCommands {
                test: Some("cargo test".into()),
                build: Some("cargo build".into()),
                lint: Some("cargo clippy".into()),
                ..ProjectCommands::default()
            },
            ..ProjectSettings::default()
        };
        let names: Vec<_> = s.commands.gates().iter().map(|g| g.0).collect();
        assert_eq!(names, ["build", "test", "lint"]);
    }

    #[test]
    fn unknown_settings_are_rejected() {
        assert!(serde_json::from_str::<ProjectSettings>(r#"{"commandz":{}}"#).is_err());
    }
}
