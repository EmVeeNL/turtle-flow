//! Approval binding documents (`docs/WORKFLOW.md` §6.2).
//!
//! An approval is bound to the SHA-256 of a canonical JSON document describing exactly
//! what was approved. Any material change produces a different hash, so a stale approval
//! can never satisfy a guard.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sha2::Digest;
use turtle_domain::{GateKind, JobId, Sha256};

pub const BINDING_SCHEMA: &str = "turtle.approval-binding.v1";

/// One bound artifact revision.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactRef {
    pub sha256: Sha256,
    pub rev: u32,
}

/// Git state bound by acceptance approvals.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitBinding {
    pub base_commit: String,
    pub head_commit: String,
    pub diff_sha256: Sha256,
    pub target_branch: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BudgetEnvelope {
    pub tokens_max: u64,
}

/// The document whose hash an approval binds to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalBinding {
    pub schema: String,
    pub gate_kind: GateKind,
    pub job_id: JobId,
    pub profile_hash: Sha256,
    pub policy_version: u32,
    /// Keyed by artifact kind (`specification`, `plan`, …). `BTreeMap` keeps keys ordered.
    pub artifacts: BTreeMap<String, ArtifactRef>,
    pub git: Option<GitBinding>,
    pub budget_envelope: Option<BudgetEnvelope>,
    /// Flags the approver explicitly acknowledged (e.g. `protected_path:Cargo.lock`), sorted.
    pub acknowledged_flags: Vec<String>,
}

impl ApprovalBinding {
    #[must_use]
    pub fn new(
        gate_kind: GateKind,
        job_id: JobId,
        profile_hash: Sha256,
        policy_version: u32,
    ) -> Self {
        Self {
            schema: BINDING_SCHEMA.to_owned(),
            gate_kind,
            job_id,
            profile_hash,
            policy_version,
            artifacts: BTreeMap::new(),
            git: None,
            budget_envelope: None,
            acknowledged_flags: Vec::new(),
        }
    }

    /// Canonical JSON: object keys sorted, no insignificant whitespace, integers only.
    /// For documents without floating-point numbers this matches RFC 8785 (JCS) output.
    #[must_use]
    pub fn canonical_json(&self) -> String {
        let mut normalized = self.clone();
        normalized.acknowledged_flags.sort();
        normalized.acknowledged_flags.dedup();
        // serde_json::Value uses a BTreeMap for objects (no `preserve_order` feature),
        // so converting through Value sorts every object's keys.
        let value = serde_json::to_value(&normalized).expect("binding is always serializable");
        serde_json::to_string(&value).expect("value is always serializable")
    }

    /// `subject_hash` stored on the approval and echoed back by the approver.
    #[must_use]
    pub fn subject_hash(&self) -> Sha256 {
        let digest = sha2::Sha256::digest(self.canonical_json().as_bytes());
        Sha256::from_bytes(digest.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sha(byte: u8) -> Sha256 {
        Sha256::from_bytes([byte; 32])
    }

    fn sample() -> ApprovalBinding {
        let id: JobId = "01JB7Y0Q4N3T9Z8K2M6W5R1C7D".parse().expect("ulid");
        let mut b = ApprovalBinding::new(GateKind::Acceptance, id, sha(1), 4);
        b.artifacts.insert(
            "specification".into(),
            ArtifactRef {
                sha256: sha(2),
                rev: 3,
            },
        );
        b.git = Some(GitBinding {
            base_commit: "3f2a9c1".into(),
            head_commit: "a81d77e".into(),
            diff_sha256: sha(3),
            target_branch: "main".into(),
        });
        b.acknowledged_flags = vec![
            "protected_path:Cargo.lock".into(),
            "dependency:governor".into(),
        ];
        b
    }

    #[test]
    fn canonical_json_has_sorted_keys_and_no_whitespace() {
        let json = sample().canonical_json();
        assert!(
            !json.contains('\n') && !json.contains("\": ") && !json.contains(", \""),
            "no formatting whitespace"
        );
        let acknowledged = json.find("\"acknowledged_flags\"").expect("key");
        let schema = json.find("\"schema\"").expect("key");
        assert!(acknowledged < schema, "keys sorted alphabetically");
        assert!(
            json.find("dependency:governor") < json.find("protected_path"),
            "flags sorted"
        );
    }

    #[test]
    fn hash_is_stable_and_order_independent() {
        let a = sample();
        let mut b = sample();
        b.acknowledged_flags.reverse();
        assert_eq!(a.subject_hash(), b.subject_hash());
    }

    #[test]
    fn any_material_change_changes_the_hash() {
        let base = sample().subject_hash();

        let mut spec_revised = sample();
        spec_revised
            .artifacts
            .get_mut("specification")
            .expect("spec")
            .rev = 4;
        assert_ne!(spec_revised.subject_hash(), base);

        let mut new_commit = sample();
        new_commit.git.as_mut().expect("git").head_commit = "b00b1e5".into();
        assert_ne!(new_commit.subject_hash(), base);

        let mut policy = sample();
        policy.policy_version = 5;
        assert_ne!(policy.subject_hash(), base);

        let mut flags = sample();
        flags.acknowledged_flags.pop();
        assert_ne!(flags.subject_hash(), base);
    }
}
