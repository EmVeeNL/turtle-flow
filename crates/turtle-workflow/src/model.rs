//! Job aggregate, commands, context, effects and events of the workflow engine.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};
use turtle_domain::{ApprovalValidity, Complexity, GateKind, JobId, JobState, Role};
use turtle_policy::MaterialChange;

use crate::profile::Profile;

// ---------------------------------------------------------------- aggregate

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PauseMode {
    /// Let the current run finish; start nothing new.
    Soft,
    /// Terminate the current run now.
    Hard,
}

/// Overlays that stop new work from being dispatched without changing the stage
/// (`docs/WORKFLOW.md` §3.1). The scheduler does not dispatch effects for held jobs.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "hold", rename_all = "snake_case")]
pub enum Hold {
    Paused { mode: PauseMode },
    AwaitingBudget,
    RateLimited { provider: String },
    WaitingForDependencies,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockReason {
    FixLimitReached,
    ReplanLimitReached,
    RespecLimitReached,
    BackTransitionLimitReached,
    PreflightDenied,
    ConflictDetected,
    Timeout,
    Unrecoverable { code: String },
}

/// Why and from where a job was blocked, and where a human may send it next.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockInfo {
    pub reason: BlockReason,
    pub from: JobState,
    pub allowed_targets: Vec<JobState>,
}

/// Loop budgets consumed by autonomous backward transitions. Missing fields read as zero, so
/// the database default `'{}'` is a valid starting value.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Counters {
    pub fix_iterations: u32,
    pub back_transitions: u32,
    pub replans: u32,
    pub respecs: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobAggregate {
    pub id: JobId,
    pub state: JobState,
    pub holds: Vec<Hold>,
    pub blocked: Option<BlockInfo>,
    /// Snapshot taken at job creation (invariant J-7: may only be escalated).
    pub profile: Profile,
    pub complexity: Option<Complexity>,
    pub counters: Counters,
    /// Plan is approved (or needs no approval) but dependencies are not delivered yet.
    pub ready_for_implementation: bool,
    /// Optimistic-concurrency version; incremented by every accepted command.
    pub version: u64,
}

impl JobAggregate {
    #[must_use]
    pub fn new(id: JobId, profile: Profile) -> Self {
        Self {
            id,
            state: JobState::Backlog,
            holds: Vec::new(),
            blocked: None,
            profile,
            complexity: None,
            counters: Counters::default(),
            ready_for_implementation: false,
            version: 0,
        }
    }

    /// The scheduler dispatches effects only for runnable jobs.
    #[must_use]
    pub fn is_runnable(&self) -> bool {
        self.holds.is_empty() && !self.state.is_terminal() && self.state != JobState::Blocked
    }

    #[must_use]
    pub fn is_paused(&self) -> bool {
        self.holds.iter().any(|h| matches!(h, Hold::Paused { .. }))
    }
}

// ---------------------------------------------------------------- commands

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewVerdict {
    Pass,
    /// Findings at or below the profile's auto-fix severity.
    Fixable,
    Replan,
    ScopeCreep,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationOutcome {
    Met,
    NotMetFixable,
    NotMetStructural,
}

/// Everything that can happen to a job. Results of agent runs and gates arrive as
/// already-evaluated facts (schema-validated, gate-checked) — never as raw agent output.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum JobCommand {
    Start,
    DiscoveryCompleted {
        report_valid: bool,
    },
    /// A specification revision was produced by the spec writer or edited by the user.
    SpecProduced {
        valid: bool,
        criteria_count: u32,
    },
    ApprovalGranted {
        gate: GateKind,
    },
    ApprovalChangesRequested {
        gate: GateKind,
    },
    ApprovalRejected {
        gate: GateKind,
    },
    ApprovalInvalidated {
        gate: GateKind,
        change: MaterialChange,
    },
    PlanProduced {
        valid: bool,
        within_scope: bool,
    },
    DependenciesDelivered,
    AllTasksSucceeded,
    /// A task exhausted its own fix loop.
    TaskFailed {
        replan_allowed: bool,
    },
    ScopeChangeDetected,
    GatesFinished {
        all_required_passed: bool,
    },
    ReviewCompleted {
        verdict: ReviewVerdict,
    },
    VerificationCompleted {
        outcome: VerificationOutcome,
    },
    Delivered,
    BaseRefreshed,
    ConflictDetected,
    ReleaseCompleted,
    /// Timeouts, unrecoverable errors and external limits.
    Halt {
        reason: BlockReason,
    },
    ResolveBlock {
        target: JobState,
    },
    MarkFailed,
    Cancel,
    Pause {
        mode: PauseMode,
    },
    Resume,
    BudgetExhausted,
    BudgetRestored,
    RateLimited {
        provider: String,
    },
    RateLimitCleared {
        provider: String,
    },
}

impl JobCommand {
    /// Stable name used in rejections, events and the transition-table snapshot.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::DiscoveryCompleted { .. } => "discovery_completed",
            Self::SpecProduced { .. } => "spec_produced",
            Self::ApprovalGranted { .. } => "approval_granted",
            Self::ApprovalChangesRequested { .. } => "approval_changes_requested",
            Self::ApprovalRejected { .. } => "approval_rejected",
            Self::ApprovalInvalidated { .. } => "approval_invalidated",
            Self::PlanProduced { .. } => "plan_produced",
            Self::DependenciesDelivered => "dependencies_delivered",
            Self::AllTasksSucceeded => "all_tasks_succeeded",
            Self::TaskFailed { .. } => "task_failed",
            Self::ScopeChangeDetected => "scope_change_detected",
            Self::GatesFinished { .. } => "gates_finished",
            Self::ReviewCompleted { .. } => "review_completed",
            Self::VerificationCompleted { .. } => "verification_completed",
            Self::Delivered => "delivered",
            Self::BaseRefreshed => "base_refreshed",
            Self::ConflictDetected => "conflict_detected",
            Self::ReleaseCompleted => "release_completed",
            Self::Halt { .. } => "halt",
            Self::ResolveBlock { .. } => "resolve_block",
            Self::MarkFailed => "mark_failed",
            Self::Cancel => "cancel",
            Self::Pause { .. } => "pause",
            Self::Resume => "resume",
            Self::BudgetExhausted => "budget_exhausted",
            Self::BudgetRestored => "budget_restored",
            Self::RateLimited { .. } => "rate_limited",
            Self::RateLimitCleared { .. } => "rate_limit_cleared",
        }
    }
}

// ---------------------------------------------------------------- context

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreflightDecision {
    Allow,
    RequireApproval,
    Deny,
}

/// Facts computed before `decide` runs (IO happens outside the engine).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecisionContext {
    /// Approval validity re-checked against a freshly computed binding hash.
    pub approvals: BTreeMap<GateKind, ApprovalValidity>,
    pub preflight: Option<PreflightDecision>,
    /// Plan exceeds thresholds (task count, estimate above approved envelope).
    pub plan_thresholds_exceeded: bool,
    pub dependencies_delivered: bool,
    /// All flags (protected paths, dependency changes) acknowledged in the acceptance approval.
    pub flags_acknowledged: bool,
    /// The `fast` auto-scope rule holds for this job (small, preflight allows, no protected paths).
    pub scope_auto_approvable: bool,
}

impl DecisionContext {
    #[must_use]
    pub fn approval(&self, gate: GateKind) -> ApprovalValidity {
        self.approvals
            .get(&gate)
            .copied()
            .unwrap_or(ApprovalValidity::Missing)
    }
}

// ---------------------------------------------------------------- outputs

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunPurpose {
    Initial,
    /// Revise after a human requested changes (feedback goes into the context package).
    Revise,
    /// Fix gate failures, review findings or unmet criteria.
    Fix,
    /// Retry after a human resolved a block.
    Retry,
}

/// Side effects requested by a decision; executed after the transaction commits.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "effect", rename_all = "snake_case")]
pub enum Effect {
    StartRun { role: Role, purpose: RunPurpose },
    RequestApproval { gate: GateKind, auto: bool },
    CreateWorktree,
    DispatchReadyTasks,
    RunGates,
    RunVerification,
    Deliver,
    CleanupWorktree,
    TerminateRuns,
    ReleaseReservations,
    SupersedePendingApprovals,
    NotifyUser,
}

impl Effect {
    /// Kinds that must run even when the job is paused, blocked or finished: they stop work,
    /// release resources or tell the user. Everything else is dropped for jobs that are
    /// blocked or finished and waits while a job has holds.
    pub const CONTROL_KINDS: &'static [&'static str] = &[
        "terminate_runs",
        "release_reservations",
        "supersede_pending_approvals",
        "notify_user",
        "cleanup_worktree",
    ];

    /// Stable name, identical to the `effect` tag in the serialized form.
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::StartRun { .. } => "start_run",
            Self::RequestApproval { .. } => "request_approval",
            Self::CreateWorktree => "create_worktree",
            Self::DispatchReadyTasks => "dispatch_ready_tasks",
            Self::RunGates => "run_gates",
            Self::RunVerification => "run_verification",
            Self::Deliver => "deliver",
            Self::CleanupWorktree => "cleanup_worktree",
            Self::TerminateRuns => "terminate_runs",
            Self::ReleaseReservations => "release_reservations",
            Self::SupersedePendingApprovals => "supersede_pending_approvals",
            Self::NotifyUser => "notify_user",
        }
    }

    #[must_use]
    pub fn is_control(&self) -> bool {
        Self::CONTROL_KINDS.contains(&self.kind())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum WorkflowEvent {
    StageEntered {
        from: JobState,
        to: JobState,
        backward: bool,
    },
    HoldAdded {
        hold: Hold,
    },
    HoldRemoved {
        hold: Hold,
    },
    Blocked {
        info: BlockInfo,
    },
    ApprovalInvalidated {
        gate: GateKind,
        change: MaterialChange,
    },
    ApprovalReRequested {
        gate: GateKind,
    },
}

/// Result of an accepted command: the new aggregate plus events and effects to persist
/// in the same transaction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decision {
    pub job: JobAggregate,
    pub events: Vec<WorkflowEvent>,
    pub effects: Vec<Effect>,
}

/// A command the engine refused. Nothing changes; `guard` names the failed rule.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Rejection {
    pub state: JobState,
    pub command: String,
    pub guard: &'static str,
}

impl fmt::Display for Rejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "command `{}` rejected in state {:?}: guard `{}` failed",
            self.command, self.state, self.guard
        )
    }
}

impl std::error::Error for Rejection {}
