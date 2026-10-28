//! Provider CLI discovery: locate the binary, read its version, check the supported range.

use std::collections::HashMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use turtle_exec::{CommandSpec, ExecError};

use crate::snapshot::{ADAPTER_VERSION, CapabilitySet, StoredSnapshot};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderId {
    ClaudeCode,
    Codex,
}

impl ProviderId {
    pub const ALL: [Self; 2] = [Self::ClaudeCode, Self::Codex];

    #[must_use]
    pub const fn binary(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude",
            Self::Codex => "codex",
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude-code",
            Self::Codex => "codex",
        }
    }

    /// CLI versions the adapter has been verified against (spike S-1/S-2, 2026-10-24).
    /// Outside this range the adapter is `Untested`, not refused.
    #[must_use]
    pub fn supported(self) -> semver::VersionReq {
        let req = match self {
            Self::ClaudeCode => ">=2.1.310, <2.2.0",
            Self::Codex => ">=0.163.0, <0.164.0",
        };
        semver::VersionReq::parse(req).expect("static version requirement is valid")
    }

    /// Parses `--version` output of the provider CLI.
    #[must_use]
    pub fn parse_version(self, output: &str) -> Option<semver::Version> {
        // claude: "2.1.310 (Claude Code)"; codex: "codex-cli 0.163.0"
        output
            .split_whitespace()
            .find_map(|token| semver::Version::parse(token.trim_start_matches('v')).ok())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompatStatus {
    Supported,
    Untested,
    NotInstalled,
    Broken,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProviderInfo {
    pub provider: ProviderId,
    pub version: Option<semver::Version>,
    pub compat: CompatStatus,
    pub detail: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthStatus {
    LoggedIn,
    NotLoggedIn,
    Unknown,
}

/// Verified static capabilities coupled to the result of noninteractive CLI discovery.
#[derive(Clone, Debug, Serialize)]
pub struct ProviderCapabilities {
    pub info: ProviderInfo,
    pub auth: AuthStatus,
    pub native_json_schema: bool,
    pub quota_visibility: String,
    pub sandbox_modes: Vec<String>,
    pub effort_levels: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct CapabilityRegistry {
    providers: HashMap<ProviderId, ProviderCapabilities>,
}

impl CapabilityRegistry {
    pub async fn discover() -> Self {
        let mut providers = HashMap::new();
        for id in ProviderId::ALL {
            providers.insert(id, discover_capabilities(id).await);
        }
        Self { providers }
    }

    /// Builds a registry from capabilities discovered elsewhere (for example with
    /// [`discover_with_snapshot`]). A provider missing from `capabilities` counts as not installed.
    #[must_use]
    pub fn from_capabilities(capabilities: impl IntoIterator<Item = ProviderCapabilities>) -> Self {
        let mut providers: HashMap<_, _> = capabilities
            .into_iter()
            .map(|caps| (caps.info.provider, caps))
            .collect();
        for id in ProviderId::ALL {
            providers.entry(id).or_insert_with(|| {
                let info = ProviderInfo {
                    provider: id,
                    version: None,
                    compat: CompatStatus::NotInstalled,
                    detail: "not discovered".into(),
                };
                let set = verified_capabilities(id, info.compat);
                ProviderCapabilities::from_set(info, AuthStatus::Unknown, set)
            });
        }
        Self { providers }
    }

    #[must_use]
    pub fn get(&self, id: ProviderId) -> &ProviderCapabilities {
        self.providers
            .get(&id)
            .expect("all provider IDs are discovered")
    }
}

impl ProviderCapabilities {
    fn from_set(info: ProviderInfo, auth: AuthStatus, set: CapabilitySet) -> Self {
        Self {
            info,
            auth,
            native_json_schema: set.native_json_schema,
            quota_visibility: set.quota_visibility,
            sandbox_modes: set.sandbox_modes,
            effort_levels: set.effort_levels,
        }
    }

    /// The version-dependent part that is persisted as a capability snapshot.
    #[must_use]
    pub fn capability_set(&self) -> CapabilitySet {
        CapabilitySet {
            adapter_version: ADAPTER_VERSION.into(),
            compat: self.info.compat,
            native_json_schema: self.native_json_schema,
            quota_visibility: self.quota_visibility.clone(),
            sandbox_modes: self.sandbox_modes.clone(),
            effort_levels: self.effort_levels.clone(),
        }
    }

    /// The CLI version a snapshot is keyed by: the parsed `--version`, if any.
    #[must_use]
    pub fn cli_version(&self) -> Option<String> {
        self.info.version.as_ref().map(ToString::to_string)
    }

    #[must_use]
    pub fn available(&self) -> bool {
        matches!(
            self.info.compat,
            CompatStatus::Supported | CompatStatus::Untested
        ) && self.auth == AuthStatus::LoggedIn
    }

    #[must_use]
    pub fn exclusion_reason(&self) -> Option<String> {
        if self.available() {
            None
        } else if matches!(
            self.info.compat,
            CompatStatus::NotInstalled | CompatStatus::Broken
        ) {
            Some(self.info.detail.clone())
        } else if self.auth == AuthStatus::NotLoggedIn {
            Some("not logged in".into())
        } else if self.auth == AuthStatus::Unknown {
            Some("login status unknown".into())
        } else {
            Some(self.info.detail.clone())
        }
    }
}

pub async fn discover_capabilities(provider: ProviderId) -> ProviderCapabilities {
    let info = discover(provider).await;
    let auth = auth_status(provider, &info).await;
    let set = verified_capabilities(provider, info.compat);
    ProviderCapabilities::from_set(info, auth, set)
}

/// Result of [`discover_with_snapshot`].
#[derive(Clone, Debug)]
pub struct Discovered {
    pub capabilities: ProviderCapabilities,
    /// The stored snapshot was reused because the CLI and adapter versions are unchanged.
    pub reused: bool,
}

/// Discovers one provider, reusing the capabilities of `stored` (its latest persisted snapshot)
/// when the CLI version and the adapter version are unchanged and `force` is not set. The
/// `--version` probe and the login check always run, so a missing or upgraded binary is never
/// hidden by a stored snapshot.
pub async fn discover_with_snapshot(
    provider: ProviderId,
    stored: Option<StoredSnapshot<'_>>,
    force: bool,
) -> Discovered {
    let info = discover(provider).await;
    let auth = auth_status(provider, &info).await;
    let live_version = info.version.as_ref().map(ToString::to_string);
    let reusable = stored
        .filter(|s| !force && live_version.is_some() && s.cli_version == live_version.as_deref())
        .and_then(|s| serde_json::from_value::<CapabilitySet>(s.capabilities.clone()).ok())
        .filter(|set| set.adapter_version == ADAPTER_VERSION && set.compat == info.compat);
    let reused = reusable.is_some();
    let set = reusable.unwrap_or_else(|| verified_capabilities(provider, info.compat));
    Discovered {
        capabilities: ProviderCapabilities::from_set(info, auth, set),
        reused,
    }
}

async fn auth_status(provider: ProviderId, info: &ProviderInfo) -> AuthStatus {
    if !matches!(
        info.compat,
        CompatStatus::Supported | CompatStatus::Untested
    ) {
        return AuthStatus::Unknown;
    }
    let args: &[&str] = match provider {
        ProviderId::ClaudeCode => &["auth", "status"],
        ProviderId::Codex => &["login", "status"],
    };
    match turtle_exec::run(
        CommandSpec::new(provider.binary())
            .args(args.iter().copied())
            .timeout(Duration::from_secs(15)),
    )
    .await
    {
        Ok(out) if out.success() => match provider {
            ProviderId::ClaudeCode
                if serde_json::from_slice::<serde_json::Value>(&out.stdout)
                    .ok()
                    .and_then(|v| v["loggedIn"].as_bool())
                    == Some(true) =>
            {
                AuthStatus::LoggedIn
            }
            ProviderId::ClaudeCode => AuthStatus::NotLoggedIn,
            ProviderId::Codex => {
                codex_auth_status(&out.stdout_lossy(), &String::from_utf8_lossy(&out.stderr))
            }
        },
        Ok(out) if provider == ProviderId::Codex => {
            codex_auth_status(&out.stdout_lossy(), &String::from_utf8_lossy(&out.stderr))
        }
        Ok(_) => AuthStatus::NotLoggedIn,
        Err(_) => AuthStatus::Unknown,
    }
}

/// Capabilities verified against the supported CLI range (spike S-1/S-2).
pub fn verified_capabilities(provider: ProviderId, compat: CompatStatus) -> CapabilitySet {
    let (quota_visibility, sandbox_modes, effort_levels): (_, &[&str], &[&str]) = match provider {
        ProviderId::ClaudeCode => (
            "partial",
            &["read-only-tools", "workspace-write"],
            &["low", "medium", "high", "xhigh", "max"],
        ),
        ProviderId::Codex => ("unknown", &["read-only", "workspace-write"], &["low"]),
    };
    let owned = |items: &[&str]| items.iter().map(|&item| item.to_owned()).collect();
    CapabilitySet {
        adapter_version: ADAPTER_VERSION.into(),
        compat,
        native_json_schema: true,
        quota_visibility: quota_visibility.into(),
        sandbox_modes: owned(sandbox_modes),
        effort_levels: owned(effort_levels),
    }
}

fn codex_auth_status(stdout: &str, stderr: &str) -> AuthStatus {
    let mut status = stdout.lines().chain(stderr.lines()).map(str::trim);
    if status.clone().any(|line| line.starts_with("Logged in")) {
        AuthStatus::LoggedIn
    } else if status.any(|line| line.starts_with("Not logged in")) {
        AuthStatus::NotLoggedIn
    } else {
        AuthStatus::Unknown
    }
}

/// Discovers one provider. Never prompts and never makes a model call.
pub async fn discover(provider: ProviderId) -> ProviderInfo {
    let spec = CommandSpec::new(provider.binary())
        .arg("--version")
        .timeout(Duration::from_secs(15));
    match turtle_exec::run(spec).await {
        Err(ExecError::NotFound(_)) => ProviderInfo {
            provider,
            version: None,
            compat: CompatStatus::NotInstalled,
            detail: format!("`{}` not found on PATH", provider.binary()),
        },
        Err(e) => ProviderInfo {
            provider,
            version: None,
            compat: CompatStatus::Broken,
            detail: e.to_string(),
        },
        Ok(out) => {
            let text = out.stdout_lossy();
            match provider.parse_version(&text) {
                Some(v) if provider.supported().matches(&v) => ProviderInfo {
                    provider,
                    version: Some(v),
                    compat: CompatStatus::Supported,
                    detail: "verified version".into(),
                },
                Some(v) => ProviderInfo {
                    provider,
                    detail: format!(
                        "outside verified range {} — run contract self-test",
                        provider.supported()
                    ),
                    version: Some(v),
                    compat: CompatStatus::Untested,
                },
                None => ProviderInfo {
                    provider,
                    version: None,
                    compat: CompatStatus::Broken,
                    detail: format!("unrecognized --version output: {:?}", text.trim()),
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_real_version_strings() {
        assert_eq!(
            ProviderId::ClaudeCode.parse_version("2.1.310 (Claude Code)\n"),
            Some(semver::Version::new(2, 1, 310))
        );
        assert_eq!(
            ProviderId::Codex.parse_version("codex-cli 0.163.0"),
            Some(semver::Version::new(0, 163, 0))
        );
        assert_eq!(ProviderId::Codex.parse_version("garbage"), None);
    }

    #[test]
    fn supported_ranges() {
        assert!(
            ProviderId::ClaudeCode
                .supported()
                .matches(&semver::Version::new(2, 1, 400))
        );
        assert!(
            !ProviderId::ClaudeCode
                .supported()
                .matches(&semver::Version::new(2, 2, 0))
        );
    }

    #[test]
    fn codex_login_status_may_be_on_stderr() {
        assert_eq!(
            codex_auth_status(
                "",
                "WARNING: aliases unavailable\nLogged in using ChatGPT\n"
            ),
            AuthStatus::LoggedIn
        );
        assert_eq!(
            codex_auth_status("Not logged in\n", ""),
            AuthStatus::NotLoggedIn
        );
        assert_eq!(
            codex_auth_status("", "WARNING: aliases unavailable\n"),
            AuthStatus::Unknown
        );
    }

    #[test]
    fn installation_and_auth_exclusions_keep_their_specific_reason() {
        let mut caps = ProviderCapabilities {
            info: ProviderInfo {
                provider: ProviderId::Codex,
                version: None,
                compat: CompatStatus::NotInstalled,
                detail: "`codex` not found on PATH".into(),
            },
            auth: AuthStatus::Unknown,
            native_json_schema: true,
            quota_visibility: "unknown".into(),
            sandbox_modes: vec!["read-only".into()],
            effort_levels: vec!["low".into()],
        };
        assert_eq!(
            caps.exclusion_reason().as_deref(),
            Some("`codex` not found on PATH")
        );
        caps.info.compat = CompatStatus::Supported;
        caps.auth = AuthStatus::NotLoggedIn;
        assert_eq!(caps.exclusion_reason().as_deref(), Some("not logged in"));
        caps.auth = AuthStatus::LoggedIn;
        assert!(caps.exclusion_reason().is_none());
    }
}
