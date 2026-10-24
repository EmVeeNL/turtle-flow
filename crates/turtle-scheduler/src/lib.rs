//! Scheduling: the engine (commands to persisted decisions), the effect dispatcher with leases
//! and heartbeats, startup recovery, and crash-injection points for tests.
//!
//! Design: `docs/ARCHITECTURE.md` §3, `docs/adr/ADR-011-job-scheduling-concurrency.md`,
//! `docs/adr/ADR-017-recovery-idempotency-crash-safety.md`.

pub mod crash;
pub mod dispatcher;
pub mod engine;
pub mod shared;

pub use crash::{CrashPlan, Crashed};
pub use dispatcher::{
    DispatchError, Dispatcher, DispatcherConfig, ExecError, ExecOutcome, Executor, TickReport,
    backoff,
};
pub use engine::{ContextProvider, Engine, EngineError, Finished, StaticContext};
pub use shared::SharedStore;
