//! Job workflow engine: the fixed stage graph, profiles, guards and the pure `decide()`
//! function (`docs/WORKFLOW.md`, ADR-005). No IO; every decision is deterministic.

pub mod decide;
pub mod model;
pub mod profile;

pub use decide::{allowed_targets, decide};
pub use model::{
    BlockInfo, BlockReason, Counters, Decision, DecisionContext, Effect, Hold, JobAggregate,
    JobCommand, PauseMode, PreflightDecision, Rejection, ReviewVerdict, RunPurpose,
    VerificationOutcome, WorkflowEvent,
};
pub use profile::{
    PlanApprovalMode, Profile, ProfileBase, ProfileError, ReviewIndependence, ScopeApprovalMode,
    StageMode,
};
