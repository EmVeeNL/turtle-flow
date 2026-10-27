//! Workflow profiles (`docs/WORKFLOW.md` §5). Profiles only *select* behavior from the fixed
//! stage graph; they cannot add states or edges. Acceptance approval is not a profile field at
//! all: delivery always requires it (`docs/adr/ADR-006-supervised-autonomy-approvals.md`).

use serde::{Deserialize, Serialize};
use turtle_domain::Complexity;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileBase {
    Fast,
    Standard,
    Strict,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StageMode {
    Off,
    /// On for complexity M and above.
    Auto,
    On,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanApprovalMode {
    Off,
    /// Required when plan thresholds are exceeded (task count, estimate above envelope).
    Threshold,
    On,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScopeApprovalMode {
    Required,
    /// `fast` only: auto-approve small jobs whose preflight allows it.
    AutoIfSmall,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewIndependence {
    Off,
    Preferred,
    Required,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    pub base: ProfileBase,
    pub discovery: StageMode,
    pub plan_approval: PlanApprovalMode,
    pub scope_approval: ScopeApprovalMode,
    pub review_independence: ReviewIndependence,
    /// Autonomous fix loops (gates, review, verification) before the job is blocked.
    pub max_fix_iterations: u32,
    /// Autonomous backward transitions per job before the job is blocked.
    pub max_back_transitions: u32,
    pub max_replans: u32,
    pub max_respecs: u32,
    pub release_required: bool,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ProfileError {
    #[error("max_fix_iterations must be between 1 and 10, got {0}")]
    FixIterations(u32),
    #[error("max_back_transitions must be at least 1")]
    BackTransitions,
    #[error("max_replans and max_respecs must be at least 1")]
    LoopLimits,
    #[error("automatic scope approval is only allowed for profiles based on `fast`")]
    AutoScopeNotFast,
    #[error("`strict` profiles must keep discovery and plan approval on")]
    StrictRelaxed,
}

impl Profile {
    #[must_use]
    pub fn fast() -> Self {
        Self {
            name: "fast".into(),
            base: ProfileBase::Fast,
            discovery: StageMode::Off,
            plan_approval: PlanApprovalMode::Off,
            scope_approval: ScopeApprovalMode::AutoIfSmall,
            review_independence: ReviewIndependence::Off,
            max_fix_iterations: 2,
            max_back_transitions: 6,
            max_replans: 3,
            max_respecs: 3,
            release_required: false,
        }
    }

    #[must_use]
    pub fn standard() -> Self {
        Self {
            name: "standard".into(),
            base: ProfileBase::Standard,
            discovery: StageMode::Auto,
            plan_approval: PlanApprovalMode::Threshold,
            scope_approval: ScopeApprovalMode::Required,
            review_independence: ReviewIndependence::Preferred,
            max_fix_iterations: 3,
            ..Self::fast()
        }
    }

    #[must_use]
    pub fn strict() -> Self {
        Self {
            name: "strict".into(),
            base: ProfileBase::Strict,
            discovery: StageMode::On,
            plan_approval: PlanApprovalMode::On,
            scope_approval: ScopeApprovalMode::Required,
            review_independence: ReviewIndependence::Required,
            max_fix_iterations: 3,
            ..Self::fast()
        }
    }

    #[must_use]
    pub fn builtin(base: ProfileBase) -> Self {
        match base {
            ProfileBase::Fast => Self::fast(),
            ProfileBase::Standard => Self::standard(),
            ProfileBase::Strict => Self::strict(),
        }
    }

    pub fn validate(&self) -> Result<(), ProfileError> {
        if !(1..=10).contains(&self.max_fix_iterations) {
            return Err(ProfileError::FixIterations(self.max_fix_iterations));
        }
        if self.max_back_transitions == 0 {
            return Err(ProfileError::BackTransitions);
        }
        if self.max_replans == 0 || self.max_respecs == 0 {
            return Err(ProfileError::LoopLimits);
        }
        if self.scope_approval == ScopeApprovalMode::AutoIfSmall && self.base != ProfileBase::Fast {
            return Err(ProfileError::AutoScopeNotFast);
        }
        if self.base == ProfileBase::Strict
            && (self.discovery != StageMode::On
                || self.plan_approval != PlanApprovalMode::On
                || self.review_independence != ReviewIndependence::Required)
        {
            return Err(ProfileError::StrictRelaxed);
        }
        Ok(())
    }

    /// Whether the discovery stage runs for a job of this complexity (unknown counts as small).
    #[must_use]
    pub fn discovery_enabled(&self, complexity: Option<Complexity>) -> bool {
        match self.discovery {
            StageMode::Off => false,
            StageMode::On => true,
            StageMode::Auto => complexity.is_some_and(|c| c >= Complexity::M),
        }
    }

    #[must_use]
    pub fn plan_approval_required(&self, thresholds_exceeded: bool) -> bool {
        match self.plan_approval {
            PlanApprovalMode::Off => false,
            PlanApprovalMode::Threshold => thresholds_exceeded,
            PlanApprovalMode::On => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_are_valid() {
        for base in [
            ProfileBase::Fast,
            ProfileBase::Standard,
            ProfileBase::Strict,
        ] {
            Profile::builtin(base)
                .validate()
                .expect("builtin profile valid");
        }
    }

    #[test]
    fn auto_scope_only_for_fast() {
        let p = Profile {
            scope_approval: ScopeApprovalMode::AutoIfSmall,
            ..Profile::standard()
        };
        assert_eq!(p.validate(), Err(ProfileError::AutoScopeNotFast));
    }

    #[test]
    fn strict_cannot_be_relaxed() {
        let p = Profile {
            plan_approval: PlanApprovalMode::Threshold,
            ..Profile::strict()
        };
        assert_eq!(p.validate(), Err(ProfileError::StrictRelaxed));
    }

    #[test]
    fn fix_iterations_bounded() {
        let p = Profile {
            max_fix_iterations: 11,
            ..Profile::standard()
        };
        assert_eq!(p.validate(), Err(ProfileError::FixIterations(11)));
    }

    #[test]
    fn discovery_auto_depends_on_complexity() {
        let p = Profile::standard();
        assert!(!p.discovery_enabled(None));
        assert!(!p.discovery_enabled(Some(Complexity::S)));
        assert!(p.discovery_enabled(Some(Complexity::M)));
    }
}
