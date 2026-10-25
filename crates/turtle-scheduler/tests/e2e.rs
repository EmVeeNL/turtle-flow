//! End-to-end: complete jobs through the real engine, dispatcher, executors, Git worktrees,
//! gates and approvals, with a scripted fake agent (milestone M1, E2E-01 and friends).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use tempfile::TempDir;
use turtle_agents::{
    AgentError, AgentOutput, AgentRequest, AgentRunner, FakeRunner, ProviderSlot, RoutingRunner,
    Step,
};
use turtle_artifacts::BlobStore;
use turtle_domain::{
    Actor, ApprovalStatus, ArtifactKind, GateKind, JobId, JobState, JobType, Priority,
    ProjectCommands, ProjectSettings, Role, TokenUsage, UsageValue,
};
use turtle_git::{Identity, Repo};
use turtle_scheduler::{
    ApprovalService, ContextConfig, CrashPlan, DispatchError, Dispatcher, DispatcherConfig, Engine,
    EngineError, ExecConfig, FactoryExecutor, RepoGitFacts, SharedStore, StoreContext,
};
use turtle_store::{
    BudgetScope, ChainVerdict, Clock, Decision, JobRecord, LedgerVerdict, ManualClock, NewJob,
    NewProject, Store, SystemClock,
};
use turtle_workflow::Profile;

type Ctx = StoreContext<RepoGitFacts>;
type Exec<R> = FactoryExecutor<R, Ctx, RepoGitFacts>;
type Disp<R> = Dispatcher<Exec<R>, Ctx>;

struct Env<R: AgentRunner> {
    _dir: TempDir,
    root: PathBuf,
    repo_path: PathBuf,
    db: PathBuf,
    blobs: Arc<BlobStore>,
    cfg: Arc<ExecConfig>,
    runner: Arc<R>,
    job: JobId,
    clock: Arc<dyn Clock>,
}

#[tokio::test]
async fn reported_runs_change_future_reservations_but_unreported_runs_do_not() {
    let usage = TokenUsage {
        input: 1_000,
        output: 200,
        ..TokenUsage::default()
    };
    let plan = serde_json::json!({"tasks": (1..=12).map(|n| serde_json::json!({
        "id": format!("T{n}"), "title": format!("Task {n}"), "instructions": "write a file",
        "expected_paths": [format!("feature_t{n}.txt")], "criteria": if n == 1 { vec!["AC-1"] } else { vec![] },
        "depends_on": [], "complexity": null
    })).collect::<Vec<_>>()});
    let runner = FakeRunner::new()
        .with_script(Role::Planner, vec![Step::Output(plan)])
        .with_usage_script(
            Role::Implementer,
            std::iter::repeat_n(UsageValue::Reported { usage }, 10)
                .chain([UsageValue::Estimated { usage }, UsageValue::Unavailable])
                .collect(),
        );
    let env = Env::new(runner, settings(Some("test -f feature_t12.txt"), None)).await;
    let (b, _) = env.boot(CrashPlan::never()).await;
    let id = env.job;
    b.store
        .with(move |s| {
            s.transaction(|tx| {
                tx.set_budget(
                    BudgetScope::Job(id),
                    5_000_000,
                    true,
                    Some(id),
                    &Actor::local_user(),
                )
            })
        })
        .await
        .expect("budget");
    let job = drive(&env, &b, &ApprovalPolicy::approve_all()).await;
    assert_eq!(job.job.state, JobState::Done);
    let events = b
        .store
        .with(move |s| s.list_events(Some(env.job)))
        .await
        .expect("events");
    let conn = Store::open(&env.db, Arc::new(SystemClock)).expect("store");
    let reservations: Vec<_> = events
        .iter()
        .filter(|e| e.kind == "ledger.reserved")
        .filter(|e| e.payload["estimate"]["basis"].is_object())
        .filter(|e| e.payload["estimate"]["p50"].is_number())
        .filter_map(|e| {
            let run = e.payload["run"].as_str()?;
            let role: String = conn
                .raw_connection()
                .query_row("SELECT role FROM agent_runs WHERE id=?1", [run], |r| {
                    r.get(0)
                })
                .ok()?;
            (role == "implementer")
                .then(|| e.payload["tokens"].as_u64())
                .flatten()
        })
        .collect();
    assert_eq!(reservations.len(), 12, "{reservations:?}");
    assert_eq!(reservations[0], 40_000);
    assert!(reservations[10] < 40_000, "{reservations:?}");
    assert_eq!(reservations[10], reservations[11]);
}

struct Booted<R: AgentRunner> {
    dispatcher: Disp<R>,
    approvals: ApprovalService<Ctx, RepoGitFacts>,
    store: SharedStore,
}

fn git_env(repo: Repo) -> Repo {
    repo.with_env("GIT_CONFIG_GLOBAL", "/dev/null")
        .with_env("GIT_CONFIG_NOSYSTEM", "1")
        .with_identity(&Identity {
            name: "Turtle Test".into(),
            email: "turtle@test.invalid".into(),
        })
}

impl<R: AgentRunner> Env<R> {
    /// A project repository on `main` with one commit, a data directory, and one job.
    async fn new(runner: R, settings: ProjectSettings) -> Self {
        Self::new_with_clock(runner, settings, Arc::new(SystemClock)).await
    }

    async fn new_with_clock(runner: R, settings: ProjectSettings, clock: Arc<dyn Clock>) -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().canonicalize().expect("canonical");
        let repo_path = root.join("project");
        std::fs::create_dir_all(&repo_path).expect("mkdir");
        let repo = git_env(Repo::new(&repo_path));
        repo.run(&["init", "-q", "-b", "main"]).await.expect("init");
        std::fs::write(repo_path.join("README.md"), "# project\n").expect("readme");
        repo.run(&["add", "-A"]).await.expect("add");
        repo.run(&["commit", "-q", "-m", "init"])
            .await
            .expect("commit");

        let mut cfg = ExecConfig::new(root.join("data/worktrees"));
        cfg.git_env = vec![
            ("GIT_CONFIG_GLOBAL".into(), "/dev/null".into()),
            ("GIT_CONFIG_NOSYSTEM".into(), "1".into()),
        ];
        cfg.identity = Some(Identity {
            name: "Turtle Test".into(),
            email: "turtle@test.invalid".into(),
        });
        let db = root.join("data/turtle.db");
        let blobs = Arc::new(BlobStore::open(root.join("data/blobs")).expect("blobs"));

        let store = SharedStore::new(Store::open(&db, Arc::clone(&clock)).expect("store"));
        let repo_str = repo_path.to_string_lossy().into_owned();
        let job = store
            .with(move |s| {
                s.transaction(|tx| {
                    let project_id = tx.insert_project(&NewProject {
                        slug: "proj".into(),
                        key_prefix: "API".into(),
                        name: "Project".into(),
                        description: String::new(),
                        repo_path: repo_str,
                        default_branch: "main".into(),
                        default_profile: "standard".into(),
                    })?;
                    tx.update_project_settings(project_id, &settings)?;
                    Ok(tx
                        .create_job(
                            &NewJob {
                                project_id,
                                title: "Add the feature".into(),
                                description: "Implement the feature file".into(),
                                job_type: JobType::Feature,
                                priority: Priority::Normal,
                                profile: Profile::standard(),
                            },
                            &Actor::local_user(),
                        )?
                        .job
                        .id)
                })
            })
            .await
            .expect("project and job");
        Self {
            _dir: dir,
            root,
            repo_path,
            db,
            blobs,
            cfg: Arc::new(cfg),
            runner: Arc::new(runner),
            job,
            clock,
        }
    }

    /// (Re)starts the whole system on the same files: a new process after a crash.
    async fn boot(&self, crash: CrashPlan) -> (Booted<R>, turtle_store::RecoveryReport) {
        let store =
            SharedStore::new(Store::open(&self.db, Arc::clone(&self.clock)).expect("store"));
        let git = Arc::new(RepoGitFacts::new(store.clone(), Arc::clone(&self.cfg)));
        let ctx = Arc::new(StoreContext::new(
            store.clone(),
            Arc::clone(&self.blobs),
            Arc::clone(&git),
            ContextConfig::default(),
        ));
        let approvals_engine = Engine::new(store.clone(), Arc::clone(&ctx), CrashPlan::never());
        let approvals = ApprovalService::new(approvals_engine, Arc::clone(&git));
        let executor = Arc::new(FactoryExecutor::new(
            store.clone(),
            Arc::clone(&self.blobs),
            Arc::clone(&self.runner),
            approvals.clone(),
            Arc::clone(&self.cfg),
        ));
        let config = DispatcherConfig {
            lease_ttl: Duration::from_secs(60),
            heartbeat: Duration::from_millis(200),
            max_concurrent: 3,
            poll_interval: Duration::from_millis(10),
        };
        let (dispatcher, report) = Dispatcher::start(store.clone(), ctx, executor, config, crash)
            .await
            .expect("start dispatcher");
        (
            Booted {
                dispatcher,
                approvals,
                store,
            },
            report,
        )
    }

    fn main_repo(&self) -> Repo {
        git_env(Repo::new(&self.repo_path))
    }

    fn worktree_dir(&self) -> PathBuf {
        self.root.join("data/worktrees/proj")
    }
}

impl<R: AgentRunner> Booted<R> {
    async fn job(&self, id: JobId) -> JobRecord {
        self.store
            .with(move |s| s.load_job(id))
            .await
            .expect("load job")
    }

    async fn unfinished(&self) -> u64 {
        self.store
            .with(|s| s.transaction(|tx| tx.unfinished_effect_count()))
            .await
            .expect("count")
    }

    /// One human action if one is due: start the job, or approve what is waiting.
    async fn human_step(&self, id: JobId, approve: &ApprovalPolicy) -> Result<(), DispatchError> {
        let job = self.job(id).await;
        let gate = match job.job.state {
            JobState::Backlog => {
                match self
                    .dispatcher
                    .engine()
                    .submit(id, turtle_workflow::JobCommand::Start, Actor::local_user())
                    .await
                {
                    Ok(_) | Err(EngineError::Rejected(_)) => {}
                    Err(e) => return Err(e.into()),
                }
                return Ok(());
            }
            JobState::ScopeApproval => GateKind::Scope,
            JobState::PlanApproval => GateKind::Plan,
            JobState::AcceptanceApproval => GateKind::Acceptance,
            _ => return Ok(()),
        };
        let pending = self
            .store
            .with(move |s| s.pending_approval(id, gate))
            .await
            .expect("query");
        let Some(a) = pending else { return Ok(()) };
        let (decision, comment) = approve.decide(gate);
        let flags = a.binding.acknowledged_flags.clone();
        match self
            .approvals
            .decide(
                a.id,
                decision,
                a.subject_hash,
                Actor::local_user(),
                "test",
                comment,
                flags,
            )
            .await
        {
            Ok(_) => {}
            Err(e) => panic!("approval failed: {e}"),
        }
        Ok(())
    }
}

/// What the simulated human decides.
struct ApprovalPolicy {
    scope_first: Decision,
}

impl ApprovalPolicy {
    fn approve_all() -> Self {
        Self {
            scope_first: Decision::Approve,
        }
    }

    fn decide(&self, gate: GateKind) -> (Decision, Option<String>) {
        if gate == GateKind::Scope && self.scope_first != Decision::Approve {
            return (self.scope_first, Some("please narrow the scope".into()));
        }
        (Decision::Approve, None)
    }
}

fn settings(test_cmd: Option<&str>, target: Option<&str>) -> ProjectSettings {
    ProjectSettings {
        commands: ProjectCommands {
            build: Some("true".into()),
            test: test_cmd.map(Into::into),
            ..ProjectCommands::default()
        },
        target_branch: target.map(Into::into),
        ..ProjectSettings::default()
    }
}

/// Runs the job until it is Done or Blocked (and all effects finished). Returns the final job.
async fn drive<R: AgentRunner>(env: &Env<R>, b: &Booted<R>, approve: &ApprovalPolicy) -> JobRecord {
    for _ in 0..600 {
        b.human_step(env.job, approve).await.expect("human step");
        let r = b.dispatcher.tick().await.expect("tick");
        let job = b.job(env.job).await;
        let finished = matches!(
            job.job.state,
            JobState::Done | JobState::Blocked | JobState::Cancelled
        );
        if finished && b.unfinished().await == 0 {
            return job;
        }
        if r.claimed == 0 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }
    panic!("job did not settle: {:?}", b.job(env.job).await.job.state);
}

async fn kinds<R: AgentRunner>(b: &Booted<R>, job: JobId) -> Vec<String> {
    b.store
        .with(move |s| s.list_events(Some(job)))
        .await
        .expect("events")
        .into_iter()
        .map(|e| e.kind)
        .collect()
}

fn exists(root: &Path, rel: &str) -> bool {
    root.join(rel).exists()
}

const NEEDS_FEATURE_FILE: &str = "test -f feature_t1.txt";

#[derive(Debug)]
struct TrackingFake {
    inner: FakeRunner,
    calls: Arc<std::sync::Mutex<Vec<Role>>>,
}

impl AgentRunner for TrackingFake {
    async fn run(&self, req: &AgentRequest) -> Result<AgentOutput, AgentError> {
        self.calls.lock().expect("calls").push(req.role);
        self.inner.run(req).await
    }
}

#[tokio::test]
async fn standard_review_routes_to_an_independent_provider_and_records_the_decision() {
    let claude_calls = Arc::new(std::sync::Mutex::new(Vec::new()));
    let codex_calls = Arc::new(std::sync::Mutex::new(Vec::new()));
    let router = RoutingRunner::new(
        ProviderSlot::new(
            TrackingFake {
                inner: FakeRunner::new(),
                calls: Arc::clone(&claude_calls),
            },
            "claude-code",
            None,
        ),
        ProviderSlot::new(
            TrackingFake {
                inner: FakeRunner::new(),
                calls: Arc::clone(&codex_calls),
            },
            "codex",
            None,
        ),
    )
    .with_role_default(Role::Reviewer, "codex");
    let env = Env::new(router, settings(Some(NEEDS_FEATURE_FILE), Some("release"))).await;
    env.main_repo()
        .run(&["branch", "release", "main"])
        .await
        .expect("branch");
    let (booted, _) = env.boot(CrashPlan::never()).await;
    let job = drive(&env, &booted, &ApprovalPolicy::approve_all()).await;
    assert_eq!(job.job.state, JobState::Done);
    let primary = claude_calls.lock().expect("calls").clone();
    let alternate = codex_calls.lock().expect("calls").clone();
    assert!(primary.contains(&Role::Implementer), "{primary:?}");
    assert!(
        !primary.contains(&Role::Reviewer) && !primary.contains(&Role::Verifier),
        "{primary:?}"
    );
    assert!(
        alternate.contains(&Role::Reviewer) && alternate.contains(&Role::Verifier),
        "{alternate:?}"
    );
    let store = Store::open(&env.db, Arc::new(SystemClock)).expect("store");
    let mut stmt = store
        .raw_connection()
        .prepare(
            "SELECT a.role,a.provider,a.model,d.chosen,d.reason,d.candidates,d.override
         FROM agent_runs a JOIN routing_decisions d ON d.id=a.routing_decision_id
         WHERE a.job_id=?1 AND a.role IN ('implementer','reviewer','verifier')",
        )
        .expect("query");
    let rows: Vec<(String, String, String, String, String, String, i64)> = stmt
        .query_map([env.job.to_string()], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
            ))
        })
        .expect("rows")
        .map(|r| r.expect("row"))
        .collect();
    assert!(
        rows.iter()
            .any(|r| r.0 == "implementer" && r.1 == "claude-code")
    );
    assert!(
        rows.iter().any(|r| r.0 == "reviewer"
            && r.1 == "codex"
            && r.4.contains("independent")
            && r.6 == 1)
    );
    assert!(rows.iter().any(|r| r.0 == "verifier" && r.1 == "codex"));
    for row in &rows {
        assert_eq!(row.3, format!("{}/{}", row.1, row.2));
        let candidates: serde_json::Value = serde_json::from_str(&row.5).expect("candidates");
        assert_eq!(candidates.as_array().expect("array").len(), 2);
    }
    let tip = env
        .main_repo()
        .branch_tip("release")
        .await
        .expect("tip")
        .expect("branch");
    let message = env
        .main_repo()
        .run(&["log", "-1", "--format=%B", &tip])
        .await
        .expect("message");
    assert!(
        message.contains("Turtle-Provider: claude-code"),
        "{message}"
    );
    let worktree = env.worktree_dir();
    let commits = env
        .main_repo()
        .run(&["log", "--all", "--format=%B", "--", "feature_t1.txt"])
        .await
        .expect("commits");
    assert!(
        commits.contains("Turtle-Provider: claude-code"),
        "{commits}; worktree={worktree:?}"
    );
}

#[tokio::test]
async fn a_mid_job_rate_limit_charges_and_reroutes_to_the_other_provider() {
    let usage = TokenUsage {
        input: 120,
        output: 30,
        ..TokenUsage::default()
    };
    let claude = FakeRunner::new().with_script(
        Role::Implementer,
        vec![Step::Fail(AgentError::rate_limited(
            "claude-code",
            Some(time::OffsetDateTime::now_utc() + time::Duration::minutes(30)),
            "usage limit reached",
            Some(UsageValue::Reported { usage }),
            Some("rate limit event".into()),
            Vec::new(),
            None,
        ))],
    );
    let router = RoutingRunner::new(
        ProviderSlot::new(claude, "claude-code", None),
        ProviderSlot::new(FakeRunner::new(), "codex", None),
    );
    let env = Env::new(router, settings(Some(NEEDS_FEATURE_FILE), Some("release"))).await;
    env.main_repo()
        .run(&["branch", "release", "main"])
        .await
        .expect("branch");
    let (booted, _) = env.boot(CrashPlan::never()).await;
    let result = drive(&env, &booted, &ApprovalPolicy::approve_all()).await;
    assert_eq!(
        result.job.state,
        JobState::Blocked,
        "required independent review cannot use the implementer's provider while Claude is limited"
    );
    let connection = Store::open(&env.db, Arc::new(SystemClock)).expect("store");
    let (failed, rerouted): (i64, i64) = connection.raw_connection().query_row(
        "SELECT sum(status='failed' AND provider='claude-code'), sum(status='succeeded' AND provider='codex') FROM agent_runs WHERE role='implementer'",
        [], |row| Ok((row.get(0)?, row.get(1)?)),
    ).expect("implementer runs");
    assert_eq!((failed, rerouted), (1, 1));
    assert!(matches!(
        connection.verify_ledger().expect("ledger"),
        LedgerVerdict::Consistent { .. }
    ));
    let open_reservations: i64 = connection.raw_connection().query_row(
        "SELECT count(*) FROM reservations r JOIN agent_runs a ON a.id=r.run_id WHERE a.job_id=?1 AND r.status='open'",
        [env.job.to_string()], |row| row.get(0),
    ).expect("reservation status");
    assert_eq!(open_reservations, 0);
    assert_eq!(
        connection
            .current_limited_providers()
            .expect("limit observation")
            .len(),
        1
    );
}

#[tokio::test]
async fn a_job_waits_for_its_rate_limit_reset_then_resumes_without_sleeping() {
    let clock = Arc::new(ManualClock::new());
    let reset_at = clock.now() + time::Duration::minutes(2);
    let runner = FakeRunner::new().with_script(
        Role::Implementer,
        vec![Step::Fail(AgentError::rate_limited(
            "fake",
            Some(reset_at),
            "test limit",
            Some(UsageValue::Reported {
                usage: TokenUsage {
                    input: 100,
                    output: 20,
                    ..TokenUsage::default()
                },
            }),
            None,
            Vec::new(),
            None,
        ))],
    );
    let env = Env::new_with_clock(
        runner,
        settings(Some(NEEDS_FEATURE_FILE), Some("release")),
        clock.clone(),
    )
    .await;
    env.main_repo()
        .run(&["branch", "release", "main"])
        .await
        .expect("branch");
    let (booted, _) = env.boot(CrashPlan::never()).await;
    for _ in 0..100 {
        booted
            .human_step(env.job, &ApprovalPolicy::approve_all())
            .await
            .expect("human step");
        booted.dispatcher.tick().await.expect("tick");
        if booted
            .job(env.job)
            .await
            .job
            .holds
            .contains(&turtle_workflow::Hold::RateLimited {
                provider: "fake".into(),
            })
        {
            break;
        }
    }
    let held = booted.job(env.job).await;
    assert!(
        held.job
            .holds
            .contains(&turtle_workflow::Hold::RateLimited {
                provider: "fake".into()
            }),
        "state={:?}, holds={:?}, blocked={:?}",
        held.job.state,
        held.job.holds,
        held.job.blocked
    );
    let connection = Store::open(&env.db, clock.clone()).expect("store");
    let not_before: String = connection.raw_connection().query_row(
        "SELECT not_before FROM effects WHERE job_id=?1 AND status='pending' AND not_before IS NOT NULL LIMIT 1",
        [env.job.to_string()], |row| row.get(0),
    ).expect("deferred effect due time");
    assert!(not_before > turtle_store::clock::format_ts(clock.now()).expect("now"));
    assert!(matches!(
        connection.verify_ledger().expect("ledger"),
        LedgerVerdict::Consistent { .. }
    ));
    assert_eq!(
        booted
            .dispatcher
            .lift_rate_limit_holds()
            .await
            .expect("early lift check"),
        0
    );
    assert!(
        booted
            .job(env.job)
            .await
            .job
            .holds
            .contains(&turtle_workflow::Hold::RateLimited {
                provider: "fake".into()
            })
    );

    clock.advance(Duration::from_secs(180));
    assert_eq!(
        booted
            .dispatcher
            .lift_rate_limit_holds()
            .await
            .expect("lift"),
        1
    );
    let resumed = drive(&env, &booted, &ApprovalPolicy::approve_all()).await;
    assert_eq!(resumed.job.state, JobState::Done);
    let after = Store::open(&env.db, clock).expect("store");
    assert!(
        after
            .current_limited_providers()
            .expect("resolved limits")
            .is_empty()
    );
    assert!(matches!(
        after.verify_ledger().expect("ledger"),
        LedgerVerdict::Consistent { .. }
    ));
    let open_reservations: i64 = after.raw_connection().query_row(
        "SELECT count(*) FROM reservations r JOIN agent_runs a ON a.id=r.run_id WHERE a.job_id=?1 AND r.status='open'",
        [env.job.to_string()], |row| row.get(0),
    ).expect("reservation status");
    assert_eq!(open_reservations, 0);
}

#[tokio::test]
async fn a_whole_job_runs_from_backlog_to_delivered() {
    let env = Env::new(
        FakeRunner::new(),
        settings(Some(NEEDS_FEATURE_FILE), Some("release")),
    )
    .await;
    env.main_repo()
        .run(&["branch", "release", "main"])
        .await
        .expect("release branch");
    let (b, _) = env.boot(CrashPlan::never()).await;

    let job = drive(&env, &b, &ApprovalPolicy::approve_all()).await;
    assert_eq!(job.job.state, JobState::Done);

    // The change reached the target branch as one squash commit with trailers...
    let repo = env.main_repo();
    let tip = repo
        .branch_tip("release")
        .await
        .expect("tip")
        .expect("release exists");
    let msg = repo
        .run(&["log", "-1", "--format=%B", &tip])
        .await
        .expect("log");
    assert!(msg.starts_with("feat(API-1): Add the feature"), "{msg}");
    assert!(
        msg.contains("Turtle-Job: API-1") && msg.contains("Turtle-Provider: fake"),
        "{msg}"
    );
    assert_eq!(
        repo.run(&["show", &format!("{tip}:feature_t1.txt")])
            .await
            .expect("show"),
        "implemented: Implement the feature"
    );
    let parents = repo
        .run(&["log", "-1", "--format=%P", &tip])
        .await
        .expect("parents");
    assert_eq!(
        parents.split_whitespace().count(),
        1,
        "a squash commit has one parent"
    );

    // ...while the user's own checkout was never touched.
    assert_eq!(
        repo.current_branch().await.expect("branch").as_deref(),
        Some("main")
    );
    assert!(repo.is_clean().await.expect("clean"));
    assert!(!exists(&env.repo_path, "feature_t1.txt"));

    // The worktree is gone, the job branch is kept for forensics.
    assert!(
        !env.worktree_dir().join("API-1").exists(),
        "worktree removed"
    );
    let branches = repo
        .run(&["branch", "--list", "turtle/*"])
        .await
        .expect("branches");
    assert!(
        branches.contains("turtle/API-1-add-the-feature"),
        "{branches}"
    );

    // Every artifact exists, approvals are recorded, runs happened once each.
    for kind in [
        ArtifactKind::Specification,
        ArtifactKind::Plan,
        ArtifactKind::ContextManifest,
        ArtifactKind::GateResult,
        ArtifactKind::ReviewReport,
        ArtifactKind::VerificationReport,
    ] {
        let id = env.job;
        let rev = b
            .store
            .with(move |s| s.latest_revision(id, kind))
            .await
            .expect("query");
        assert!(rev.is_some(), "{kind:?} missing");
    }
    let id = env.job;
    let context_links = b
        .store
        .with(move |s| s.run_context_manifest_revisions(id))
        .await
        .expect("context links");
    assert!(
        context_links.len() >= 5,
        "expected context on every agent run: {context_links:?}"
    );
    assert!(
        context_links.iter().all(Option::is_some),
        "every run should link its manifest: {context_links:?}"
    );
    let id = env.job;
    let manifest_revisions = b
        .store
        .with(move |s| s.list_revisions(id, ArtifactKind::ContextManifest))
        .await
        .expect("manifest revisions");
    let revision_ids: std::collections::BTreeSet<_> = manifest_revisions
        .into_iter()
        .map(|revision| revision.id)
        .collect();
    assert!(
        context_links
            .iter()
            .flatten()
            .all(|revision| revision_ids.contains(revision))
    );
    let id = env.job;
    let approvals = b
        .store
        .with(move |s| s.list_approvals(id))
        .await
        .expect("approvals");
    for gate in [GateKind::Scope, GateKind::Acceptance] {
        let a = approvals
            .iter()
            .find(|a| a.gate == gate && a.status == ApprovalStatus::Approved)
            .expect("approved");
        assert_eq!(a.decided_by.as_deref(), Some("local"));
    }
    let delivery = b
        .store
        .with(move |s| s.delivery(id))
        .await
        .expect("query")
        .expect("delivery recorded");
    assert_eq!(
        (delivery.status.as_str(), delivery.target_branch.as_str()),
        ("delivered", "release")
    );
    for role in [
        Role::SpecWriter,
        Role::Planner,
        Role::Implementer,
        Role::Reviewer,
        Role::Verifier,
    ] {
        assert_eq!(env.runner.calls_of(role), 1, "{role:?}");
    }
    assert_eq!(env.runner.calls_of(Role::Fixer), 0);

    // The audit trail is intact and nothing failed.
    assert!(matches!(
        b.store.with(|s| s.verify_chain()).await.expect("verify"),
        ChainVerdict::Intact { .. }
    ));
    let k = kinds(&b, env.job).await;
    assert!(
        !k.contains(&"effect.failed".to_owned()) && k.contains(&"job.delivery_recorded".to_owned()),
        "{k:?}"
    );
}

#[tokio::test]
async fn delivery_to_a_checked_out_branch_is_parked_for_the_user() {
    let env = Env::new(FakeRunner::new(), settings(Some(NEEDS_FEATURE_FILE), None)).await;
    let (b, _) = env.boot(CrashPlan::never()).await;
    let job = drive(&env, &b, &ApprovalPolicy::approve_all()).await;
    assert_eq!(job.job.state, JobState::Done);

    let repo = env.main_repo();
    let id = env.job;
    let delivery = b
        .store
        .with(move |s| s.delivery(id))
        .await
        .expect("query")
        .expect("recorded");
    assert_eq!(delivery.status, "pending_integration");
    assert!(
        !exists(&env.repo_path, "feature_t1.txt"),
        "main was not moved"
    );
    assert!(repo.is_clean().await.expect("clean"));
    repo.run(&["merge", "--ff-only", "-q", "refs/turtle/delivery/API-1"])
        .await
        .expect("the documented command works");
    assert!(exists(&env.repo_path, "feature_t1.txt"));
}

#[tokio::test]
async fn failing_checks_send_the_job_through_the_fix_loop() {
    // The first implementation forgets the file the project's test requires.
    let runner = FakeRunner::new().with_script(
        Role::Implementer,
        vec![Step::WriteFile {
            path: "unrelated.txt".into(),
            content: "oops\n".into(),
        }],
    );
    let env = Env::new(runner, settings(Some(NEEDS_FEATURE_FILE), Some("release"))).await;
    env.main_repo()
        .run(&["branch", "release", "main"])
        .await
        .expect("release branch");
    let (b, _) = env.boot(CrashPlan::never()).await;

    let job = drive(&env, &b, &ApprovalPolicy::approve_all()).await;
    assert_eq!(job.job.state, JobState::Done);
    assert_eq!(env.runner.calls_of(Role::Fixer), 1, "one fix iteration");
    assert!(
        env.runner
            .calls()
            .iter()
            .any(|c| c.role == Role::Fixer && c.had_feedback),
        "the fixer was told what failed"
    );
    assert_eq!(job.job.counters.fix_iterations, 1);

    let repo = env.main_repo();
    let tip = repo
        .branch_tip("release")
        .await
        .expect("tip")
        .expect("release");
    assert!(
        repo.run(&["show", &format!("{tip}:feature_t1.txt")])
            .await
            .is_ok(),
        "the fixer's work was delivered"
    );
    let k = kinds(&b, env.job).await;
    assert!(k.iter().filter(|x| *x == "job.stage_entered").count() > 10);
}

#[tokio::test]
async fn blocking_review_findings_are_fixed_before_verification() {
    let review = serde_json::json!({
        "summary": "needs work",
        "findings": [{"id": "R-1", "severity": "major", "category": "correctness", "message": "handle the empty case"}]
    });
    let runner = FakeRunner::new().with_script(Role::Reviewer, vec![Step::Output(review)]);
    let env = Env::new(runner, settings(Some(NEEDS_FEATURE_FILE), Some("release"))).await;
    env.main_repo()
        .run(&["branch", "release", "main"])
        .await
        .expect("release branch");
    let (b, _) = env.boot(CrashPlan::never()).await;

    let job = drive(&env, &b, &ApprovalPolicy::approve_all()).await;
    assert_eq!(job.job.state, JobState::Done);
    assert_eq!(
        env.runner.calls_of(Role::Reviewer),
        2,
        "reviewed again after the fix"
    );
    assert_eq!(env.runner.calls_of(Role::Fixer), 1);
}

#[tokio::test]
async fn an_agent_that_commits_on_its_own_is_stopped_and_undone() {
    let runner = FakeRunner::new().with_script(Role::Implementer, vec![Step::CommitInWorktree]);
    let env = Env::new(runner, settings(Some(NEEDS_FEATURE_FILE), Some("release"))).await;
    env.main_repo()
        .run(&["branch", "release", "main"])
        .await
        .expect("release branch");
    let (b, _) = env.boot(CrashPlan::never()).await;

    let job = drive(&env, &b, &ApprovalPolicy::approve_all()).await;
    assert_eq!(
        job.job.state,
        JobState::Blocked,
        "a policy violation never continues silently"
    );
    let info = job.job.blocked.expect("block info");
    assert!(
        matches!(&info.reason, turtle_workflow::BlockReason::Unrecoverable { code } if code == "effect_failed:dispatch_ready_tasks"),
        "{:?}",
        info.reason
    );

    let repo = env.main_repo();
    // The agent's commit is preserved for inspection but not on the job branch.
    let refs = repo
        .run(&[
            "for-each-ref",
            "--format=%(refname)",
            "refs/turtle/recovery/",
        ])
        .await
        .expect("refs");
    assert_eq!(refs.lines().count(), 1, "one recovery ref: {refs}");
    let recovery = refs.lines().next().expect("ref");
    assert_eq!(
        repo.run(&["show", &format!("{recovery}:sneaky.txt")])
            .await
            .expect("show")
            .trim(),
        "committed by the agent"
    );
    let wt = repo.at(env.worktree_dir().join("API-1"));
    let base = job.base_commit.expect("base commit");
    assert_eq!(
        wt.head().await.expect("head"),
        base,
        "the job branch is back at its base"
    );
    assert!(wt.is_clean().await.expect("clean"));
    // Nothing was delivered.
    assert_eq!(
        repo.branch_tip("release")
            .await
            .expect("tip")
            .expect("release"),
        base
    );
    assert_eq!(
        env.runner.calls_of(Role::Reviewer),
        0,
        "review never ran on tampered work"
    );
}

#[tokio::test]
async fn requested_changes_reach_the_spec_writer_and_the_job_continues() {
    let env = Env::new(
        FakeRunner::new(),
        settings(Some(NEEDS_FEATURE_FILE), Some("release")),
    )
    .await;
    env.main_repo()
        .run(&["branch", "release", "main"])
        .await
        .expect("release branch");
    let (b, _) = env.boot(CrashPlan::never()).await;

    // First scope decision: request changes. Then approve everything.
    let first = ApprovalPolicy {
        scope_first: Decision::RequestChanges,
    };
    for _ in 0..200 {
        b.human_step(env.job, &first).await.expect("human");
        b.dispatcher.tick().await.expect("tick");
        if env.runner.calls_of(Role::SpecWriter) >= 2 {
            break;
        }
    }
    assert_eq!(
        env.runner.calls_of(Role::SpecWriter),
        2,
        "the spec is written again"
    );
    assert!(
        env.runner
            .calls()
            .iter()
            .filter(|c| c.role == Role::SpecWriter)
            .any(|c| c.had_feedback),
        "the second run received the reviewer's comment"
    );
    let job = drive(&env, &b, &ApprovalPolicy::approve_all()).await;
    assert_eq!(job.job.state, JobState::Done);
}

#[tokio::test]
async fn a_failing_agent_is_retried_then_blocks_the_job() {
    let runner = FakeRunner::new().with_script(
        Role::SpecWriter,
        (0..10)
            .map(|_| Step::Fail(AgentError::Fatal("not logged in".into())))
            .collect(),
    );
    let env = Env::new(runner, settings(Some(NEEDS_FEATURE_FILE), Some("release"))).await;
    let (b, _) = env.boot(CrashPlan::never()).await;
    let job = drive(&env, &b, &ApprovalPolicy::approve_all()).await;
    assert_eq!(job.job.state, JobState::Blocked);
    assert_eq!(job.job.blocked.expect("info").from, JobState::Specification);
    assert_eq!(
        env.runner.calls_of(Role::SpecWriter),
        1,
        "a fatal error is not retried"
    );
}

#[tokio::test]
async fn the_end_to_end_run_survives_a_crash_at_any_step_boundary() {
    // Baseline first, to learn how many step boundaries a clean run has and what it produces.
    async fn run(crashes: &[usize]) -> (Vec<String>, usize, usize, usize, usize, Vec<usize>) {
        let env = Env::new(
            FakeRunner::new(),
            settings(Some(NEEDS_FEATURE_FILE), Some("release")),
        )
        .await;
        env.main_repo()
            .run(&["branch", "release", "main"])
            .await
            .expect("release branch");
        let mut plans = crashes.iter().map(|&n| CrashPlan::at(n));
        let first = plans.next().unwrap_or_else(CrashPlan::never);
        let (mut b, _) = env.boot(first.clone()).await;
        let id = env.job;
        b.store
            .with(move |s| {
                s.transaction(|tx| {
                    tx.set_budget(
                        BudgetScope::Job(id),
                        50_000_000,
                        true,
                        Some(id),
                        &Actor::local_user(),
                    )
                })
            })
            .await
            .expect("budget");
        let mut restarts = 0;
        for _ in 0..800 {
            let step = async {
                b.human_step(env.job, &ApprovalPolicy::approve_all())
                    .await?;
                b.dispatcher.tick().await?;
                Ok::<_, DispatchError>(())
            }
            .await;
            match step {
                Ok(()) => {}
                Err(DispatchError::Crashed(_) | DispatchError::Engine(EngineError::Crashed(_))) => {
                    restarts += 1;
                    drop(b);
                    let (nb, _) = env
                        .boot(plans.next().unwrap_or_else(CrashPlan::never))
                        .await;
                    b = nb;
                }
                Err(e) => panic!("unexpected: {e}"),
            }
            let job = b.job(env.job).await;
            if job.job.state == JobState::Done && b.unfinished().await == 0 {
                break;
            }
        }
        let job = b.job(env.job).await;
        assert_eq!(job.job.state, JobState::Done, "crashes {crashes:?}");
        // However often the daemon died mid-run, no reservation leaks and the cached budget
        // columns still equal the ledger.
        let (verdict, budgets) = b
            .store
            .with(|s| Ok::<_, turtle_store::StoreError>((s.verify_ledger()?, s.list_budgets()?)))
            .await
            .expect("ledger");
        assert!(
            matches!(verdict, LedgerVerdict::Consistent { .. }),
            "crashes {crashes:?}: {verdict:?}"
        );
        assert!(
            budgets.iter().all(|b| b.reserved_tokens == 0),
            "crashes {crashes:?}: leaked reservation {budgets:?}"
        );
        assert!(
            matches!(
                b.store.with(|s| s.verify_chain()).await.expect("verify"),
                ChainVerdict::Intact { .. }
            ),
            "crashes {crashes:?}"
        );

        let repo = env.main_repo();
        // Exactly one delivery commit and one commit per task: no duplicated side effects.
        let deliveries = repo
            .run(&[
                "log",
                "--format=%H",
                "-E",
                "--grep",
                "^Turtle-Job: API-1$",
                "release",
            ])
            .await
            .expect("log");
        let task_commits = repo
            .run(&[
                "log",
                "--format=%H",
                "-E",
                "--grep",
                "^Turtle-Task: ",
                "turtle/API-1-add-the-feature",
            ])
            .await
            .expect("log");
        let id = env.job;
        let revs = b
            .store
            .with(move |s| {
                [
                    ArtifactKind::Specification,
                    ArtifactKind::Plan,
                    ArtifactKind::ReviewReport,
                    ArtifactKind::VerificationReport,
                ]
                .map(|k| s.list_revisions(id, k).expect("revisions").len())
            })
            .await;
        let events = kinds(&b, env.job)
            .await
            .into_iter()
            .filter(|k| k.starts_with("job.stage_entered"))
            .count();
        let calls = [
            Role::SpecWriter,
            Role::Planner,
            Role::Implementer,
            Role::Reviewer,
            Role::Verifier,
            Role::Fixer,
        ]
        .map(|r| env.runner.calls_of(r))
        .to_vec();
        (
            vec![format!("{revs:?}")],
            deliveries.lines().count(),
            task_commits.lines().count(),
            events,
            restarts,
            calls,
        )
    }

    let (base_revs, base_deliveries, base_tasks, base_stages, _, base_calls) = run(&[]).await;
    assert_eq!(base_calls, [1, 1, 1, 1, 1, 0]);
    assert_eq!((base_deliveries, base_tasks), (1, 1));

    // Learn the boundaries of a clean run, then crash at *every* point where an effect has just
    // finished its work (the dangerous moment: paid-for work done, completion not recorded) and
    // at a spread of the other kinds.
    let names = {
        let env = Env::new(
            FakeRunner::new(),
            settings(Some(NEEDS_FEATURE_FILE), Some("release")),
        )
        .await;
        env.main_repo()
            .run(&["branch", "release", "main"])
            .await
            .expect("release branch");
        let plan = CrashPlan::never();
        let (b, _) = env.boot(plan.clone()).await;
        drive(&env, &b, &ApprovalPolicy::approve_all()).await;
        plan.boundary_names()
    };
    assert!(names.len() >= 30, "{}", names.len());
    let executed: Vec<usize> = names
        .iter()
        .enumerate()
        .filter(|(_, n)| n.starts_with("effect:executed"))
        .map(|(i, _)| i)
        .collect();
    assert!(
        executed.len() >= 10,
        "expected one per effect, got {}",
        executed.len()
    );
    let others = (0..names.len())
        .filter(|i| !executed.contains(i))
        .step_by(7);
    let mut crashed = 0;
    for n in executed.iter().copied().chain(others) {
        let (revs, deliveries, tasks, stages, restarts, calls) = run(&[n]).await;
        assert_eq!(restarts, 1, "boundary {n} ({})", names[n]);
        assert_eq!(
            deliveries, 1,
            "boundary {n} ({}): delivered exactly once",
            names[n]
        );
        assert_eq!(
            tasks, 1,
            "boundary {n} ({}): exactly one commit per task",
            names[n]
        );
        assert_eq!(
            revs, base_revs,
            "boundary {n} ({}): no phantom artifact revisions",
            names[n]
        );
        assert_eq!(
            stages, base_stages,
            "boundary {n} ({}): every stage entered exactly as often as in a clean run",
            names[n]
        );
        assert_eq!(
            calls, base_calls,
            "boundary {n} ({}): no agent was paid for twice",
            names[n]
        );
        crashed += 1;
    }
    assert!(crashed >= 6);
}

#[tokio::test]
async fn a_budget_that_cannot_cover_the_next_run_holds_the_job_until_it_is_raised() {
    let env = Env::new(
        FakeRunner::new(),
        settings(Some(NEEDS_FEATURE_FILE), Some("release")),
    )
    .await;
    env.main_repo()
        .run(&["branch", "release", "main"])
        .await
        .expect("release branch");
    let (b, _) = env.boot(CrashPlan::never()).await;
    let id = env.job;
    b.store
        .with(move |s| {
            s.transaction(|tx| {
                tx.set_budget(
                    BudgetScope::Job(id),
                    5_000,
                    true,
                    Some(id),
                    &Actor::local_user(),
                )
            })
        })
        .await
        .expect("tiny budget");

    // Start the job and let the dispatcher run until the first agent run is refused.
    for _ in 0..200 {
        b.human_step(id, &ApprovalPolicy::approve_all())
            .await
            .expect("human step");
        b.dispatcher.tick().await.expect("tick");
        if !b.job(id).await.job.holds.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let held = b.job(id).await;
    assert_eq!(held.job.holds, vec![turtle_workflow::Hold::AwaitingBudget]);
    assert_ne!(held.job.state, JobState::Blocked, "a hold is not a failure");
    assert_eq!(env.runner.calls().len(), 0, "no agent was paid for");
    assert!(kinds(&b, id).await.contains(&"ledger.denied".to_owned()));

    // Ticking while held does nothing, and the refused effect is waiting, not failed.
    for _ in 0..5 {
        b.dispatcher.tick().await.expect("tick");
    }
    assert!(b.unfinished().await > 0);
    assert_eq!(env.runner.calls().len(), 0);

    // The user raises the budget and lifts the hold; the same effect then runs.
    b.store
        .with(move |s| {
            s.transaction(|tx| {
                tx.set_budget(
                    BudgetScope::Job(id),
                    5_000_000,
                    true,
                    Some(id),
                    &Actor::local_user(),
                )
            })
        })
        .await
        .expect("raise");
    assert!(
        turtle_scheduler::lift_budget_hold(b.dispatcher.engine(), id)
            .await
            .expect("lift"),
        "the budget has room again"
    );

    let job = drive(&env, &b, &ApprovalPolicy::approve_all()).await;
    assert_eq!(job.job.state, JobState::Done);
    let (verdict, budgets, usage) = b
        .store
        .with(move |s| {
            Ok::<_, turtle_store::StoreError>((
                s.verify_ledger()?,
                s.list_budgets()?,
                s.job_usage(id)?,
            ))
        })
        .await
        .expect("ledger");
    assert!(matches!(verdict, LedgerVerdict::Consistent { .. }));
    assert_eq!(budgets[0].reserved_tokens, 0);
    // The fake agent states 1,200 tokens per run (as an estimate, not a measurement), and every
    // run was settled with exactly that.
    assert_eq!(usage.tokens_estimated, 1_200 * usage.runs);
    assert_eq!(
        budgets[0].settled_tokens,
        usage.tokens_reported + usage.tokens_estimated
    );
    assert!(usage.runs >= 5, "{usage:?}");
}

#[tokio::test]
async fn a_failed_reported_run_is_charged_but_does_not_train_and_keeps_a_redacted_transcript() {
    let runner = FakeRunner::new().with_script(
        Role::SpecWriter,
        vec![Step::Fail(AgentError::Fatal("provider stopped after usage".into()).with_details(
            Some(UsageValue::Reported { usage: TokenUsage { input: 400, output: 75, ..TokenUsage::default() } }),
            Some(r#"{"type":"result","api_key":"ghp_123456789012345678901234567890123456"}"#.into()),
            vec![turtle_agents::PermissionDenial { tool: "Bash".into(), input: serde_json::json!({"command":"git commit token=ghp_123456789012345678901234567890123456"}) }],
        ))],
    );
    let env = Env::new(runner, settings(Some(NEEDS_FEATURE_FILE), Some("release"))).await;
    env.main_repo()
        .run(&["branch", "release", "main"])
        .await
        .expect("release branch");
    let (b, _) = env.boot(CrashPlan::never()).await;
    let id = env.job;
    b.store
        .with(move |s| {
            s.transaction(|tx| {
                tx.set_budget(
                    BudgetScope::Job(id),
                    5_000_000,
                    true,
                    Some(id),
                    &Actor::local_user(),
                )
            })
        })
        .await
        .expect("budget");
    let job = drive(&env, &b, &ApprovalPolicy::approve_all()).await;
    assert_eq!(job.job.state, JobState::Blocked);

    let (budgets, usage, verdict, events, run) = b
        .store
        .with(move |s| {
            Ok::<_, turtle_store::StoreError>((
                s.list_budgets()?,
                s.job_usage(id)?,
                s.verify_ledger()?,
                s.list_events(Some(id))?,
                s.runs_for_job(id)?,
            ))
        })
        .await
        .expect("ledger");
    assert!(matches!(verdict, LedgerVerdict::Consistent { .. }));
    assert_eq!(budgets[0].reserved_tokens, 0);
    assert_eq!(usage.runs_unavailable, 0);
    assert_eq!(usage.tokens_reported, 475);
    assert_eq!(run[0].provenance, "reported");
    assert_eq!(
        events
            .iter()
            .filter(|event| event.kind == "run.policy_denied")
            .count(),
        1
    );
    let transcript_hash = run[0].transcript_blob.expect("stored run transcript");
    let transcript = String::from_utf8(env.blobs.get(&transcript_hash).expect("read transcript"))
        .expect("utf8 transcript");
    assert!(!transcript.contains("ghp_123456789012345678901234567890123456"));
    let observations: i64 = b
        .store
        .with(|s| {
            Ok::<_, turtle_store::StoreError>(s.raw_connection().query_row(
                "SELECT count(*) FROM estimation_observations",
                [],
                |row| row.get(0),
            )?)
        })
        .await
        .expect("observations");
    assert_eq!(observations, 0, "failed usage must not train the estimator");
    assert_eq!(
        budgets[0].settled_tokens,
        usage.tokens_reported + usage.tokens_estimated,
        "reported failed usage is charged"
    );
}

#[tokio::test]
async fn a_failed_run_without_usage_remains_unknown_and_releases_its_reservation() {
    let runner = FakeRunner::new().with_script(
        Role::SpecWriter,
        vec![Step::Fail(AgentError::Fatal(
            "provider failed before reporting usage".into(),
        ))],
    );
    let env = Env::new(runner, settings(Some(NEEDS_FEATURE_FILE), Some("release"))).await;
    env.main_repo()
        .run(&["branch", "release", "main"])
        .await
        .expect("release branch");
    let (b, _) = env.boot(CrashPlan::never()).await;
    let id = env.job;
    b.store
        .with(move |s| {
            s.transaction(|tx| {
                tx.set_budget(
                    BudgetScope::Job(id),
                    5_000_000,
                    true,
                    Some(id),
                    &Actor::local_user(),
                )
            })
        })
        .await
        .expect("budget");
    assert_eq!(
        drive(&env, &b, &ApprovalPolicy::approve_all())
            .await
            .job
            .state,
        JobState::Blocked
    );
    let (budgets, usage, verdict) = b
        .store
        .with(move |s| {
            Ok::<_, turtle_store::StoreError>((
                s.list_budgets()?,
                s.job_usage(id)?,
                s.verify_ledger()?,
            ))
        })
        .await
        .expect("ledger");
    assert!(matches!(verdict, LedgerVerdict::Consistent { .. }));
    assert_eq!(budgets[0].reserved_tokens, 0);
    assert_eq!(usage.runs_unavailable, 1);
    assert_eq!(
        budgets[0].settled_tokens,
        usage.tokens_reported + usage.tokens_estimated
    );
}
