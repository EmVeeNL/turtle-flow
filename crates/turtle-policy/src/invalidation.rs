//! Which changes invalidate which approvals (`docs/WORKFLOW.md` §6.3).
//!
//! Invalidation is applied eagerly when a change happens, and defensively at every guard by
//! recomputing the binding hash. This table drives the eager path.

use serde::{Deserialize, Serialize};
use turtle_domain::GateKind;

/// A change that may invalidate approvals.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaterialChange {
    /// New specification revision (agent or user).
    SpecRevised,
    PlanRevised,
    /// New commit or working-tree change in the job worktree.
    WorktreeChanged,
    /// Job branch rebased or merged onto a moved base.
    BaseRefreshed,
    ProfileEscalated,
    /// Policy version changed in a way that affects the job (e.g. new protected path).
    PolicyChanged,
    /// Actual plus projected usage exceeds the approved budget envelope.
    BudgetEnvelopeExceeded,
}

impl MaterialChange {
    /// Gates whose approvals become invalid after this change.
    #[must_use]
    pub const fn invalidates(self) -> &'static [GateKind] {
        use GateKind::{Acceptance, Plan, Scope};
        match self {
            Self::SpecRevised => &[Scope, Plan, Acceptance],
            Self::PlanRevised => &[Plan, Acceptance],
            Self::WorktreeChanged | Self::BaseRefreshed | Self::PolicyChanged => &[Acceptance],
            Self::ProfileEscalated => &[Scope, Plan],
            // The scope itself is not re-reviewed; a budget-increase approval is requested instead.
            Self::BudgetEnvelopeExceeded => &[],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_revision_invalidates_everything_downstream() {
        assert_eq!(
            MaterialChange::SpecRevised.invalidates(),
            [GateKind::Scope, GateKind::Plan, GateKind::Acceptance]
        );
    }

    #[test]
    fn code_changes_only_invalidate_acceptance() {
        for change in [
            MaterialChange::WorktreeChanged,
            MaterialChange::BaseRefreshed,
        ] {
            assert_eq!(change.invalidates(), [GateKind::Acceptance]);
        }
    }
}
