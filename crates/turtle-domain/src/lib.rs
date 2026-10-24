//! Core domain types for Turtle Flow: typed IDs, job keys, workflow states, roles,
//! token usage with provenance, and provider quota observations.
//!
//! This crate performs no IO. See `docs/DOMAIN-MODEL.md` for the full model and invariants.

pub mod approval;
pub mod documents;
pub mod hash;
pub mod ids;
pub mod job;
pub mod paths;
pub mod quota;
pub mod reports;
pub mod roles;
pub mod settings;
pub mod usage;

pub use approval::{ApprovalStatus, ApprovalValidity, GateKind};
pub use documents::{
    AcceptanceCriterion, ArtifactKind, Plan, PlanError, PlanTask, SpecError, Specification,
    VerificationMethod,
};
pub use hash::{HashParseError, Sha256};
pub use ids::{
    ApprovalId, ArtifactId, BudgetId, EffectId, EventId, JobId, ProjectId, ReservationId, RunId,
    TaskId,
};
pub use job::{JobKey, JobKeyError, JobState, JobType, Priority};
pub use quota::{QuotaStatus, QuotaWindow, WindowStatus};
pub use reports::{
    CriterionResult, CriterionStatus, Finding, ReviewOutcome, ReviewReport, Severity,
    VerificationDecision, VerificationReport,
};
pub use roles::{Actor, Complexity, Role, TrustLevel};
pub use settings::{ProjectCommands, ProjectSettings};
pub use usage::{TokenUsage, UsageProvenance, UsageValue};
