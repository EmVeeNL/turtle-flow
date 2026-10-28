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
pub mod estimate;
pub mod executor;
pub mod git_facts;
pub mod layout;
pub mod pr;
pub mod project;
pub mod rate_limit;
pub mod shared;
pub mod static_gates;
pub mod tasks;

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
pub use project::{AddProject, ProjectError, ProjectService};
pub use rate_limit::lift_rate_limit_holds;
pub use shared::SharedStore;
