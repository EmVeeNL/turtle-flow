//! Scheduling: the engine (commands to persisted decisions), the effect dispatcher with leases
//! and heartbeats, startup recovery, and crash-injection points for tests.
//!
//! Design: `docs/ARCHITECTURE.md` §3, `docs/adr/ADR-011-job-scheduling-concurrency.md`,
//! `docs/adr/ADR-017-recovery-idempotency-crash-safety.md`.

pub mod approvals;
pub mod binding;
pub mod budget;
pub mod context;
pub mod crash;
pub mod dispatcher;
pub mod engine;
pub mod executor;
pub mod git_facts;
pub mod layout;
pub mod shared;

pub use approvals::{ApprovalError, ApprovalService};
pub use binding::{
    ArtifactSnapshot, BindingError, GitFacts, GitFactsProvider, NoGit, POLICY_VERSION,
    build_binding, profile_hash,
};
pub use budget::lift_budget_hold;
pub use context::{ContextConfig, StoreContext};
pub use crash::{CrashPlan, Crashed};
pub use dispatcher::{
    DispatchError, Dispatcher, DispatcherConfig, ExecError, ExecOutcome, Executor, TickReport,
    backoff,
};
pub use engine::{ContextProvider, Engine, EngineError, Finished, StaticContext};
pub use executor::FactoryExecutor;
pub use git_facts::RepoGitFacts;
pub use layout::{ExecConfig, Layout};
pub use shared::SharedStore;
