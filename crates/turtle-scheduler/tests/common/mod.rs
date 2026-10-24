//! Shared test world: a simulated executor that "performs" effects against an in-memory world
//! and drives the workflow forward by returning the command each effect would produce.
#![allow(dead_code)]

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::Notify;
use turtle_domain::{Actor, ApprovalValidity, GateKind, JobId, JobState, JobType, Priority};
use turtle_scheduler::{
    ContextProvider, CrashPlan, Dispatcher, DispatcherConfig, ExecError, ExecOutcome, Executor,
    SharedStore, StaticContext,
};
use turtle_store::{EffectRow, NewJob, NewProject, Store};
use turtle_workflow::{
    DecisionContext, JobCommand as C, PreflightDecision, Profile, ReviewVerdict,
    VerificationOutcome,
};

pub fn permissive() -> DecisionContext {
    let mut cx = DecisionContext {
        preflight: Some(PreflightDecision::Allow),
        dependencies_delivered: true,
        flags_acknowledged: true,
        ..DecisionContext::default()
    };
    for g in GateKind::ALL {
        cx.approvals.insert(g, ApprovalValidity::Valid);
    }
    cx
}

pub fn fast_config() -> DispatcherConfig {
    DispatcherConfig {
        lease_ttl: Duration::from_millis(400),
        heartbeat: Duration::from_millis(20),
        max_concurrent: 3,
        poll_interval: Duration::from_millis(10),
    }
}

/// What the simulated outside world recorded.
#[derive(Debug, Default)]
pub struct World {
    /// idempotency key -> number of times an executor ran for it
    pub executions: Mutex<BTreeMap<String, u32>>,
    /// effect kinds in the order they were first executed
    pub order: Mutex<Vec<String>>,
}

impl World {
    pub fn executions_of(&self, key: &str) -> u32 {
        self.executions
            .lock()
            .expect("lock")
            .get(key)
            .copied()
            .unwrap_or(0)
    }
    pub fn keys(&self) -> Vec<String> {
        self.executions
            .lock()
            .expect("lock")
            .keys()
            .cloned()
            .collect()
    }
}

#[derive(Default)]
pub struct Faults {
    /// Errors returned before any work, per effect kind, consumed front to back.
    pub errors: HashMap<String, Vec<ExecError>>,
    /// Panics once for these kinds.
    pub panic_once: Vec<String>,
}

/// Blocks `start_run` effects until released, to model long-running work.
#[derive(Clone)]
pub struct Gate {
    pub started: Arc<Notify>,
    pub release: Arc<Notify>,
}

pub struct SimExecutor {
    pub world: Arc<World>,
    pub faults: Mutex<Faults>,
    pub gate: Mutex<Option<Gate>>,
}

impl SimExecutor {
    pub fn new(world: Arc<World>) -> Arc<Self> {
        Arc::new(Self {
            world,
            faults: Mutex::new(Faults::default()),
            gate: Mutex::new(None),
        })
    }

    pub fn fail(&self, kind: &str, errors: Vec<ExecError>) {
        self.faults
            .lock()
            .expect("lock")
            .errors
            .insert(kind.into(), errors);
    }
}

fn script(effect: &EffectRow) -> Option<C> {
    let role = effect.payload.get("role").and_then(|r| r.as_str());
    Some(match (effect.kind.as_str(), role) {
        ("start_run", Some("spec_writer")) => C::SpecProduced {
            valid: true,
            criteria_count: 3,
        },
        ("start_run", Some("planner")) => C::PlanProduced {
            valid: true,
            within_scope: true,
        },
        ("start_run", Some("reviewer")) => C::ReviewCompleted {
            verdict: ReviewVerdict::Pass,
        },
        ("dispatch_ready_tasks", _) => C::AllTasksSucceeded,
        ("run_gates", _) => C::GatesFinished {
            all_required_passed: true,
        },
        ("run_verification", _) => C::VerificationCompleted {
            outcome: VerificationOutcome::Met,
        },
        ("deliver", _) => C::Delivered,
        _ => return None,
    })
}

impl Executor for SimExecutor {
    async fn execute(&self, effect: &EffectRow) -> Result<ExecOutcome, ExecError> {
        let (should_panic, injected) = {
            let mut f = self.faults.lock().expect("lock");
            let should_panic = f
                .panic_once
                .iter()
                .position(|k| *k == effect.kind)
                .map(|i| f.panic_once.remove(i))
                .is_some();
            let injected = f
                .errors
                .get_mut(&effect.kind)
                .filter(|list| !list.is_empty())
                .map(|list| list.remove(0));
            (should_panic, injected)
        };
        // Panic only after the lock is released, so the fault table stays usable.
        assert!(!should_panic, "simulated executor bug");
        if let Some(err) = injected {
            return Err(err);
        }
        let gate = self.gate.lock().expect("lock").clone();
        if let Some(g) = gate
            && effect.kind == "start_run"
        {
            g.started.notify_one();
            g.release.notified().await;
        }
        // The idempotent "world": running again after a crash is observable but harmless.
        let first = {
            let mut e = self.world.executions.lock().expect("lock");
            let n = e.entry(effect.idempotency_key.clone()).or_insert(0);
            *n += 1;
            *n == 1
        };
        if first {
            self.world
                .order
                .lock()
                .expect("lock")
                .push(effect.kind.clone());
        }
        let outcome = ExecOutcome::done(serde_json::json!({ "kind": effect.kind }));
        Ok(match script(effect) {
            Some(cmd) => outcome.with_command(cmd),
            None => outcome,
        })
    }
}

pub type TestDispatcher = Dispatcher<SimExecutor, StaticContext>;

pub fn store_with_job(store: Store) -> (SharedStore, JobId) {
    let shared = SharedStore::new(store);
    (shared, JobId::default())
}

/// Creates a project and a job in `backlog`; returns the job id.
pub async fn create_job(store: &SharedStore, profile: Profile) -> JobId {
    store
        .with(move |s| {
            s.transaction(|tx| {
                let project_id = tx.insert_project(&NewProject {
                    slug: "proj".into(),
                    key_prefix: "API".into(),
                    name: "P".into(),
                    description: String::new(),
                    repo_path: "/tmp/p".into(),
                    default_branch: "main".into(),
                    default_profile: "standard".into(),
                })?;
                let rec = tx.create_job(
                    &NewJob {
                        project_id,
                        title: "t".into(),
                        description: String::new(),
                        job_type: JobType::Feature,
                        priority: Priority::Normal,
                        profile,
                    },
                    &Actor::local_user(),
                )?;
                Ok(rec.job.id)
            })
        })
        .await
        .expect("create job")
}

pub async fn state_of(d: &TestDispatcher, job: JobId) -> JobState {
    d.engine()
        .store()
        .with(move |s| s.load_job(job))
        .await
        .expect("load")
        .job
        .state
}

pub async fn start_dispatcher(
    store: &SharedStore,
    exec: &Arc<SimExecutor>,
    crash: CrashPlan,
) -> (TestDispatcher, turtle_store::RecoveryReport) {
    Dispatcher::start(
        store.clone(),
        Arc::new(StaticContext(permissive())),
        Arc::clone(exec),
        fast_config(),
        crash,
    )
    .await
    .expect("start dispatcher")
}

/// A context provider is just a trait; this helper keeps tests readable.
pub fn _assert_provider<T: ContextProvider>() {}
