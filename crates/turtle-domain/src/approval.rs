//! Approval gate kinds and statuses (`docs/WORKFLOW.md` §6, `docs/DOMAIN-MODEL.md` §3.6).

use serde::{Deserialize, Serialize};

/// What an approval decides about.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GateKind {
    Scope,
    Plan,
    BudgetIncrease,
    ScopeChange,
    PolicyException,
    Acceptance,
    Release,
}

impl GateKind {
    pub const ALL: [Self; 7] = [
        Self::Scope,
        Self::Plan,
        Self::BudgetIncrease,
        Self::ScopeChange,
        Self::PolicyException,
        Self::Acceptance,
        Self::Release,
    ];

    /// Only scope approvals may ever be granted automatically, and only under `fast` rules
    /// (`docs/WORKFLOW.md` §6.4). Acceptance and release always need a human.
    #[must_use]
    pub const fn may_auto_approve(self) -> bool {
        matches!(self, Self::Scope)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalStatus {
    Pending,
    Approved,
    Rejected,
    ChangesRequested,
    Expired,
    Invalidated,
    Superseded,
}

/// Result of re-checking an approval at guard time (invariant AP-4): the stored decision
/// *and* a recomputed binding hash that still matches.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalValidity {
    Valid,
    Missing,
    /// Approved once, but the binding no longer matches, it expired, or its HMAC failed.
    Invalid,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_scope_can_be_auto_approved() {
        let auto: Vec<_> = GateKind::ALL
            .into_iter()
            .filter(|g| g.may_auto_approve())
            .collect();
        assert_eq!(auto, [GateKind::Scope]);
    }
}
