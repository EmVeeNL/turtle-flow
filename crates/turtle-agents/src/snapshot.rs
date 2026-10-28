//! Versioned capability snapshots: the version-dependent part of discovery that the composition
//! root persists in `provider_capabilities`, reuses while the CLI version is unchanged, and diffs
//! in `turtle doctor` after a provider CLI upgrade. Login status is not part of a snapshot; it is
//! checked on every discovery.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::CompatStatus;

/// Version of the adapter that produced a snapshot. A stored snapshot from another adapter
/// version is never reused, because the verified capabilities may have changed with it.
pub const ADAPTER_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilitySet {
    pub adapter_version: String,
    pub compat: CompatStatus,
    pub native_json_schema: bool,
    pub quota_visibility: String,
    pub sandbox_modes: Vec<String>,
    pub effort_levels: Vec<String>,
}

impl CapabilitySet {
    #[must_use]
    pub fn to_json(&self) -> Value {
        serde_json::to_value(self).expect("capability set is always serializable")
    }
}

/// The latest persisted snapshot of one provider, as discovery may reuse it.
#[derive(Clone, Copy, Debug)]
pub struct StoredSnapshot<'a> {
    pub cli_version: Option<&'a str>,
    pub capabilities: &'a Value,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct VersionChange {
    pub from: Option<String>,
    pub to: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CapabilityChange {
    pub capability: String,
    pub from: Value,
    pub to: Value,
}

/// Differences between two snapshots. List members are compared as sets and reported as
/// `capability:member`; other values that differ are `changed`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct CapabilityDiff {
    pub cli_version: Option<VersionChange>,
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub changed: Vec<CapabilityChange>,
}

impl CapabilityDiff {
    #[must_use]
    pub fn between(old_cli: Option<&str>, old: &Value, new_cli: Option<&str>, new: &Value) -> Self {
        let mut diff = Self {
            cli_version: (old_cli != new_cli).then(|| VersionChange {
                from: old_cli.map(str::to_owned),
                to: new_cli.map(str::to_owned),
            }),
            ..Self::default()
        };
        let empty = serde_json::Map::new();
        let old = old.as_object().unwrap_or(&empty);
        let new = new.as_object().unwrap_or(&empty);
        let keys: BTreeSet<&String> = old.keys().chain(new.keys()).collect();
        for key in keys {
            match (old.get(key), new.get(key)) {
                (None, Some(_)) => diff.added.push(key.clone()),
                (Some(_), None) => diff.removed.push(key.clone()),
                (Some(Value::Array(from)), Some(Value::Array(to))) if from != to => {
                    let label = |v: &Value| v.as_str().map_or_else(|| v.to_string(), str::to_owned);
                    let from: BTreeSet<String> = from.iter().map(label).collect();
                    let to: BTreeSet<String> = to.iter().map(label).collect();
                    diff.added
                        .extend(to.difference(&from).map(|m| format!("{key}:{m}")));
                    diff.removed
                        .extend(from.difference(&to).map(|m| format!("{key}:{m}")));
                }
                (Some(from), Some(to)) if from != to => diff.changed.push(CapabilityChange {
                    capability: key.clone(),
                    from: from.clone(),
                    to: to.clone(),
                }),
                _ => {}
            }
        }
        diff
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.cli_version.is_none()
            && self.added.is_empty()
            && self.removed.is_empty()
            && self.changed.is_empty()
    }

    /// One line for the doctor table, e.g. `cli 2.1.310 → 2.1.311; +effort_levels:ultra`.
    #[must_use]
    pub fn summary(&self) -> String {
        if self.is_empty() {
            return "no changes".into();
        }
        let unknown = || "none".to_owned();
        let mut parts = Vec::new();
        if let Some(change) = &self.cli_version {
            parts.push(format!(
                "cli {} → {}",
                change.from.clone().unwrap_or_else(unknown),
                change.to.clone().unwrap_or_else(unknown)
            ));
        }
        parts.extend(self.added.iter().map(|a| format!("+{a}")));
        parts.extend(self.removed.iter().map(|r| format!("-{r}")));
        parts.extend(self.changed.iter().map(|c| {
            let show = |v: &Value| v.as_str().map_or_else(|| v.to_string(), str::to_owned);
            format!("{} {} → {}", c.capability, show(&c.from), show(&c.to))
        }));
        parts.join("; ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_cli_upgrade_reports_version_members_and_scalar_changes() {
        let old = json!({"compat": "supported", "effort_levels": ["low", "high"], "quota_visibility": "partial"});
        let new = json!({"compat": "untested", "effort_levels": ["low", "high", "max"], "image_input": true});
        let diff = CapabilityDiff::between(Some("2.1.310"), &old, Some("2.2.0"), &new);
        assert_eq!(
            diff.cli_version,
            Some(VersionChange {
                from: Some("2.1.310".into()),
                to: Some("2.2.0".into())
            })
        );
        assert_eq!(diff.added, ["effort_levels:max", "image_input"]);
        assert_eq!(diff.removed, ["quota_visibility"]);
        assert_eq!(diff.changed[0].capability, "compat");
        assert_eq!(
            diff.summary(),
            "cli 2.1.310 → 2.2.0; +effort_levels:max; +image_input; -quota_visibility; compat supported → untested"
        );
        let same = CapabilityDiff::between(Some("2.2.0"), &new, Some("2.2.0"), &new);
        assert!(same.is_empty());
        assert_eq!(same.summary(), "no changes");
    }
}
