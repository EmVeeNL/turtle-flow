//! Shared helpers for workflow tests.
#![allow(dead_code)]

use turtle_domain::{Actor, ApprovalValidity, GateKind, JobId, JobState};
use turtle_workflow::{
    BlockInfo, BlockReason, DecisionContext, JobAggregate, JobCommand, PauseMode,
    PreflightDecision, Profile, ReviewVerdict, VerificationOutcome, allowed_targets,
};

pub fn job_id() -> JobId {
    "01JB7Y0Q4N3T9Z8K2M6W5R1C7D".parse().expect("valid ulid")
}

pub fn job_in(state: JobState, profile: Profile) -> JobAggregate {
    let mut job = JobAggregate::new(job_id(), profile);
    job.state = state;
    if state == JobState::Blocked {
        job.blocked = Some(BlockInfo {
            reason: BlockReason::FixLimitReached,
            from: JobState::BuildTest,
            allowed_targets: allowed_targets(JobState::BuildTest),
        });
    }
    job
}

/// Every guard passes.
pub fn permissive() -> DecisionContext {
    DecisionContext {
        approvals: GateKind::ALL
            .into_iter()
            .map(|g| (g, ApprovalValidity::Valid))
            .collect(),
        preflight: Some(PreflightDecision::Allow),
        plan_thresholds_exceeded: false,
        dependencies_delivered: true,
        flags_acknowledged: true,
        scope_auto_approvable: false,
    }
}

/// Nothing is approved, nothing is computed.
pub fn hostile() -> DecisionContext {
    DecisionContext::default()
}

pub fn user() -> Actor {
    Actor::local_user()
}

pub fn scheduler() -> Actor {
    Actor::Scheduler
}

/// One representative instance of every command variant, plus important parameter variants.
pub fn sample_commands() -> Vec<JobCommand> {
    use JobCommand as C;
    vec![
        C::Start,
        C::DiscoveryCompleted { report_valid: true },
        C::DiscoveryCompleted {
            report_valid: false,
        },
        C::SpecProduced {
            valid: true,
            criteria_count: 4,
        },
        C::SpecProduced {
            valid: true,
            criteria_count: 0,
        },
        C::ApprovalGranted {
            gate: GateKind::Scope,
        },
        C::ApprovalGranted {
            gate: GateKind::Plan,
        },
        C::ApprovalGranted {
            gate: GateKind::Acceptance,
        },
        C::ApprovalChangesRequested {
            gate: GateKind::Scope,
        },
        C::ApprovalChangesRequested {
            gate: GateKind::Acceptance,
        },
        C::ApprovalRejected {
            gate: GateKind::Scope,
        },
        C::ApprovalInvalidated {
            gate: GateKind::Acceptance,
            change: turtle_policy::MaterialChange::WorktreeChanged,
        },
        C::PlanProduced {
            valid: true,
            within_scope: true,
        },
        C::PlanProduced {
            valid: true,
            within_scope: false,
        },
        C::DependenciesDelivered,
        C::AllTasksSucceeded,
        C::TaskFailed {
            replan_allowed: true,
        },
        C::TaskFailed {
            replan_allowed: false,
        },
        C::ScopeChangeDetected,
        C::GatesFinished {
            all_required_passed: true,
        },
        C::GatesFinished {
            all_required_passed: false,
        },
        C::ReviewCompleted {
            verdict: ReviewVerdict::Pass,
        },
        C::ReviewCompleted {
            verdict: ReviewVerdict::Fixable,
        },
        C::ReviewCompleted {
            verdict: ReviewVerdict::Replan,
        },
        C::ReviewCompleted {
            verdict: ReviewVerdict::ScopeCreep,
        },
        C::VerificationCompleted {
            outcome: VerificationOutcome::Met,
        },
        C::VerificationCompleted {
            outcome: VerificationOutcome::NotMetFixable,
        },
        C::VerificationCompleted {
            outcome: VerificationOutcome::NotMetStructural,
        },
        C::Delivered,
        C::BaseRefreshed,
        C::ConflictDetected,
        C::ReleaseCompleted,
        C::Halt {
            reason: BlockReason::Timeout,
        },
        C::ResolveBlock {
            target: JobState::Implementation,
        },
        C::ResolveBlock {
            target: JobState::Delivery,
        },
        C::MarkFailed,
        C::Cancel,
        C::Pause {
            mode: PauseMode::Soft,
        },
        C::Pause {
            mode: PauseMode::Hard,
        },
        C::Resume,
        C::BudgetExhausted,
        C::BudgetRestored,
        C::BudgetIncreaseApproved,
        C::RateLimited {
            provider: "codex".into(),
        },
        C::RateLimitCleared {
            provider: "codex".into(),
        },
    ]
}

/// The actor that normally issues a command.
pub fn natural_actor(cmd: &JobCommand) -> Actor {
    use JobCommand as C;
    match cmd {
        C::Start
        | C::ApprovalGranted { .. }
        | C::ApprovalChangesRequested { .. }
        | C::ApprovalRejected { .. }
        | C::ResolveBlock { .. }
        | C::MarkFailed
        | C::Cancel
        | C::Pause { .. }
        | C::Resume => user(),
        _ => scheduler(),
    }
}
