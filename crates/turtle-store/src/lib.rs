//! SQLite persistence: migrations, hash-chained event log, effect outbox, job repository.
//! The only crate that issues SQL (architecture invariant I-1). Design: `docs/DATA-MODEL.md`,
//! `docs/adr/ADR-004-sqlite-persistence-event-logging.md`.

pub mod approvals;
pub mod artifacts;
pub mod chain;
pub mod clock;
pub mod dashboard;
pub mod dashboard_reads;
pub mod error;
pub mod estimation;
pub mod integrity;
pub mod ledger;
pub mod migrate;
pub mod projects;
pub mod providers;
pub mod queue;
pub mod resources;
pub mod store;
pub mod tasks;

pub use approvals::{ApprovalRecord, Decision};
pub use artifacts::{NewRevision, RevisionRecord};
pub use clock::{Clock, ManualClock, StepClock, SystemClock};
pub use dashboard_reads::{DashboardEstimate, ProjectStats};
pub use error::{Result, StoreError};
pub use ledger::{
    BudgetRecord, BudgetScope, JobUsage, LedgerVerdict, NewRoutingDecision, NewRun,
    ReservationOutcome, RunEnd, RunRecord,
};
pub use projects::{DeliveryRecord, DeliveryStatus, ProjectRecord};
pub use providers::{
    CapabilitySnapshot, LimitObservation, QuotaSnapshot, capability_hash, read_capability_snapshots,
};
pub use queue::{ClaimParams, EffectFate, RecoveryReport};
pub use resources::{NewResource, ResourceHit, ResourceRecord};
pub use store::{
    Applied, ChainVerdict, EffectRow, EventRow, JobRecord, NewEvent, NewJob, NewProject,
    RebuiltJob, Store, Tx,
};
pub use tasks::{
    NewGateResult, NewTask, TaskRecord, TaskState, TaskSummary, TrailerCommit, task_id,
};
