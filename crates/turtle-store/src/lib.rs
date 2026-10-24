//! SQLite persistence: migrations, hash-chained event log, effect outbox, job repository.
//! The only crate that issues SQL (architecture invariant I-1). Design: `docs/DATA-MODEL.md`,
//! `docs/adr/ADR-004-sqlite-persistence-event-logging.md`.

pub mod chain;
pub mod clock;
pub mod error;
pub mod migrate;
pub mod queue;
pub mod store;

pub use clock::{Clock, ManualClock, StepClock, SystemClock};
pub use error::{Result, StoreError};
pub use queue::{ClaimParams, EffectFate, RecoveryReport};
pub use store::{
    Applied, ChainVerdict, EffectRow, EventRow, JobRecord, NewEvent, NewJob, NewProject,
    RebuiltJob, Store, Tx,
};
