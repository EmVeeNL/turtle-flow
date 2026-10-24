//! Provider CLI discovery: locate the binary, read its version, check the supported range.

use std::time::Duration;

use serde::Serialize;
use turtle_exec::{CommandSpec, ExecError};

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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
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
}
