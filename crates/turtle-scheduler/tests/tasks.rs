//! Plan tasks through the real engine, dispatcher, executor and Git: the `tasks` table drives
//! per-task gates, links runs, gate results and commits, and restart recovery reads it instead
//! of the commit trailers, which must agree with it.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use tempfile::TempDir;
use turtle_agents::{AgentRunner, FakeRunner, ProviderSlot, RoutingRunner, Step};
use turtle_artifacts::BlobStore;
use turtle_domain::{
    Actor, GateKind, JobId, JobState, JobType, Priority, ProjectCommands, ProjectSettings, Role,
};
use turtle_git::{Identity, Repo};
use turtle_scheduler::{
    ApprovalService, ContextConfig, CrashPlan, Dispatcher, DispatcherConfig, Engine, EngineError,
    ExecConfig, Executor, FactoryExecutor, RepoGitFacts, SharedStore, StoreContext,
};
use turtle_store::{
    ChainVerdict, Decision, EffectRow, JobRecord, NewJob, NewProject, Store, SystemClock,
    TaskRecord, TaskState,
};
use turtle_workflow::{BlockReason, JobCommand, Profile};

type Ctx = StoreContext<RepoGitFacts>;
type Exec<R> = FactoryExecutor<R, Ctx, RepoGitFacts>;

const BRANCH: &str = "turtle/API-1-add-the-feature";

struct Env<R: AgentRunner> {
    _dir: TempDir,
    repo_path: PathBuf,
    db: PathBuf,
    blobs: Arc<BlobStore>,
    cfg: Arc<ExecConfig>,
    runner: Arc<R>,
    job: JobId,
}

struct Booted<R: AgentRunner> {
    dispatcher: Dispatcher<Exec<R>, Ctx>,
    executor: Arc<Exec<R>>,
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

fn settings(test: Option<&str>, lint: bool) -> ProjectSettings {
    ProjectSettings {
        commands: ProjectCommands {
            build: Some("true".into()),
            test: test.map(Into::into),
            lint: lint.then(|| "true".into()),
            ..ProjectCommands::default()
        },
        ..ProjectSettings::default()
    }
}

/// Two tasks; T2 depends on T1.
fn two_task_plan() -> Value {
    json!({"tasks": [
        {"id": "T1", "title": "First part", "instructions": "write the first file",
         "expected_paths": ["feature_t1.txt"], "criteria": ["AC-1"], "depends_on": []},
        {"id": "T2", "title": "Second part", "instructions": "write the second file",
         "expected_paths": ["feature_t2.txt"], "criteria": [], "depends_on": ["T1"]}
    ]})
}

impl<R: AgentRunner> Env<R> {
    async fn new(runner: R, settings: ProjectSettings, profile: Profile) -> Self {
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
        let mut store = Store::open(&db, Arc::new(SystemClock)).expect("store");
        let repo_str = repo_path.to_string_lossy().into_owned();
        let job = store
            .transaction(|tx| {
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
                            profile,
                        },
                        &Actor::local_user(),
                    )?
                    .job
                    .id)
            })
            .expect("project and job");
        Self {
            _dir: dir,
            repo_path,
            db,
            blobs,
            cfg: Arc::new(cfg),
            runner: Arc::new(runner),
            job,
        }
    }

    /// (Re)starts the system on the same files, as a new process would after a crash.
    async fn boot(&self) -> Booted<R> {
        let store = SharedStore::new(Store::open(&self.db, Arc::new(SystemClock)).expect("store"));
        let git = Arc::new(RepoGitFacts::new(store.clone(), Arc::clone(&self.cfg)));
        let ctx = Arc::new(StoreContext::new(
            store.clone(),
            Arc::clone(&self.blobs),
            Arc::clone(&git),
            ContextConfig::default(),
        ));
        let approvals = ApprovalService::new(
            Engine::new(store.clone(), Arc::clone(&ctx), CrashPlan::never()),
            Arc::clone(&git),
        );
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
        let (dispatcher, _) = Dispatcher::start(
            store.clone(),
            ctx,
            Arc::clone(&executor),
            config,
            CrashPlan::never(),
        )
        .await
        .expect("start dispatcher");
        Booted {
            dispatcher,
            executor,
            approvals,
            store,
        }
    }

    fn repo(&self) -> Repo {
        git_env(Repo::new(&self.repo_path))
    }

    /// Direct access, as a user (or a bug) editing the database would have.
    fn sql(&self, statement: &str, task: &str) {
        let store = Store::open(&self.db, Arc::new(SystemClock)).expect("store");
        store
            .raw_connection()
            .execute(statement, [task])
            .expect("tamper");
    }
}

impl<R: AgentRunner> Booted<R> {
    async fn job(&self, id: JobId) -> JobRecord {
        self.store
            .with(move |s| s.load_job(id))
            .await
            .expect("load job")
    }

    async fn tasks(&self, id: JobId) -> Vec<TaskRecord> {
        self.store
            .with(move |s| s.tasks_for_job(id))
            .await
            .expect("tasks")
    }

    async fn events(&self, id: JobId) -> Vec<turtle_store::EventRow> {
        self.store
            .with(move |s| s.list_events(Some(id)))
            .await
            .expect("events")
    }

    /// Start the job or approve whatever it waits for.
    async fn human_step(&self, id: JobId) {
        let gate = match self.job(id).await.job.state {
            JobState::Backlog => {
                match self
                    .dispatcher
                    .engine()
                    .submit(id, JobCommand::Start, Actor::local_user())
                    .await
                {
                    Ok(_) | Err(EngineError::Rejected(_)) => {}
                    Err(e) => panic!("start: {e}"),
                }
                return;
            }
            JobState::ScopeApproval => GateKind::Scope,
            JobState::PlanApproval => GateKind::Plan,
            JobState::AcceptanceApproval => GateKind::Acceptance,
            _ => return,
        };
        let Some(a) = self
            .store
            .with(move |s| s.pending_approval(id, gate))
            .await
            .expect("query")
        else {
            return;
        };
        self.approvals
            .decide(
                a.id,
                Decision::Approve,
                a.subject_hash,
                Actor::local_user(),
                "test",
                None,
                a.binding.acknowledged_flags.clone(),
            )
            .await
            .expect("approve");
    }

    /// Runs until the job is in `until` (with nothing left to do when it is final).
    async fn run_until(&self, id: JobId, until: JobState, approve_acceptance: bool) -> JobRecord {
        for _ in 0..600 {
            let job = self.job(id).await;
            if job.job.state == until {
                return job;
            }
            if approve_acceptance || job.job.state != JobState::AcceptanceApproval {
                self.human_step(id).await;
            }
            let ticked = self.dispatcher.tick().await.expect("tick");
            if ticked.claimed == 0 {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }
        panic!(
            "job did not reach {until:?}: {:?}",
            self.job(id).await.job.state
        );
    }

    async fn dispatch_effect(&self, id: JobId) -> EffectRow {
        self.store
            .with(move |s| s.list_effects(id))
            .await
            .expect("effects")
            .into_iter()
            .rfind(|e| e.kind == "dispatch_ready_tasks")
            .expect("dispatch effect")
    }

    /// Delivers the dispatch effect again, as the dispatcher does after a crash.
    async fn redispatch(&self, id: JobId) -> Option<JobCommand> {
        let effect = self.dispatch_effect(id).await;
        self.executor
            .execute(&effect)
            .await
            .expect("re-execution")
            .command
    }
}

fn runs_of(env_db: &std::path::Path, role: &str) -> i64 {
    Store::open(env_db, Arc::new(SystemClock))
        .expect("store")
        .raw_connection()
        .query_row(
            "SELECT count(*) FROM agent_runs WHERE role = ?1",
            [role],
            |r| r.get(0),
        )
        .expect("count")
}

#[tokio::test]
async fn strict_task_gates_move_each_task_through_the_table_and_link_everything() {
    // Both providers share one script: whichever implements T1 first changes nothing, so its
    // strict gates fail once and a fixer repairs it.
    let runner = || {
        FakeRunner::new()
            .with_script(Role::Planner, vec![Step::Output(two_task_plan())])
            .with_script(Role::Implementer, vec![Step::Skip])
    };
    let router = RoutingRunner::new(
        ProviderSlot::new(runner(), "claude-code", None),
        ProviderSlot::new(runner(), "codex", None),
    );
    let env = Env::new(
        router,
        settings(Some("test -f feature_t1.txt"), true),
        Profile::strict(),
    )
    .await;
    let b = env.boot().await;
    let job = b.run_until(env.job, JobState::Done, true).await;
    assert_eq!(job.job.state, JobState::Done);

    let tasks = b.tasks(env.job).await;
    assert_eq!(tasks.len(), 2);
    let (t1, t2) = (&tasks[0], &tasks[1]);
    assert_eq!(
        (t1.plan_task_id.as_str(), t2.plan_task_id.as_str()),
        ("T1", "T2")
    );
    assert!(tasks.iter().all(|t| t.state == TaskState::Succeeded));
    assert_eq!((t1.fix_iterations, t2.fix_iterations), (1, 0));
    assert_eq!(t2.depends_on, std::slice::from_ref(&t1.id));

    // Git keeps its own record: each task commit's trailer is the task's row id.
    let trailers = env
        .repo()
        .task_commits("main", BRANCH)
        .await
        .expect("task commits");
    assert_eq!(trailers.len(), 2);
    for (task, commit) in tasks.iter().zip(&trailers) {
        assert_eq!(commit.task, task.id);
        assert_eq!(task.commit_sha.as_deref(), Some(commit.sha.as_str()));
    }

    let store = Store::open(&env.db, Arc::new(SystemClock)).expect("store");
    let conn = store.raw_connection();
    let per_task = |sql: &str, task: &str| -> i64 {
        conn.query_row(sql, [task], |r| r.get(0)).expect("count")
    };
    let runs = "SELECT count(*) FROM agent_runs WHERE task_id = ?1";
    assert_eq!(per_task(runs, &t1.id), 2, "implementer and fixer");
    assert_eq!(per_task(runs, &t2.id), 1);
    let stage_runs: i64 = conn
        .query_row(
            "SELECT count(*) FROM agent_runs WHERE task_id IS NULL AND role IN ('implementer','fixer')",
            [],
            |r| r.get(0),
        )
        .expect("count");
    assert_eq!(stage_runs, 0);
    let gates = "SELECT count(*) FROM gate_results WHERE task_id = ?1";
    let failed = "SELECT count(*) FROM gate_results WHERE task_id = ?1 AND outcome = 'failed'";
    // build, test, lint, secret scan and scope conformance per attempt.
    assert_eq!(per_task(gates, &t1.id), 10);
    assert_eq!(per_task(failed, &t1.id), 1, "the first test attempt");
    assert_eq!(per_task(gates, &t2.id), 5);
    let records = "SELECT count(*) FROM commit_records WHERE kind = 'task' AND task_id = ?1";
    assert_eq!(
        (per_task(records, &t1.id), per_task(records, &t2.id)),
        (1, 1)
    );
    let (passed_tree, head): (String, String) = conn
        .query_row(
            "SELECT tree_hash, head_commit FROM gate_results WHERE task_id = ?1 AND gate = 'test'
             ORDER BY rowid DESC LIMIT 1",
            [&t1.id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .expect("gate row");
    let repo = env.repo();
    let sha = t1.commit_sha.as_deref().expect("commit");
    assert_eq!(
        passed_tree,
        repo.tree_of(sha).await.expect("tree"),
        "gates are bound to what was committed"
    );
    assert_eq!(head, repo.rev(&format!("{sha}^")).await.expect("parent"));

    let moves: Vec<String> = b
        .events(env.job)
        .await
        .into_iter()
        .filter(|e| e.kind == "task.state_changed" && e.payload["task"] == t1.id.as_str())
        .map(|e| {
            format!(
                "{}>{}",
                e.payload["from"].as_str().unwrap_or("?"),
                e.payload["to"].as_str().unwrap_or("?")
            )
        })
        .collect();
    assert_eq!(
        moves,
        [
            "ready>running",
            "running>gating",
            "gating>fixing",
            "fixing>gating",
            "gating>succeeded"
        ]
    );
    assert!(matches!(
        store.verify_chain().expect("verify"),
        ChainVerdict::Intact { .. }
    ));
}

#[tokio::test]
async fn restart_recovery_reads_the_table_and_reports_disagreeing_trailers() {
    // T1 changes nothing, so it has no commit and no trailer: only the table knows it is done.
    let runner = FakeRunner::new()
        .with_script(Role::Planner, vec![Step::Output(two_task_plan())])
        .with_script(Role::Implementer, vec![Step::Skip]);
    let env = Env::new(runner, settings(None, false), Profile::standard()).await;
    let b = env.boot().await;
    b.run_until(env.job, JobState::AcceptanceApproval, false)
        .await;
    let tasks = b.tasks(env.job).await;
    let (t1, t2) = (tasks[0].clone(), tasks[1].clone());
    assert_eq!(
        (t1.state, t1.commit_sha.as_deref()),
        (TaskState::Succeeded, None)
    );
    let commit = t2.commit_sha.clone().expect("T2 committed");
    assert_eq!(env.runner.calls_of(Role::Implementer), 2);

    // A restart delivers the effect again: nothing is re-run, not even the task without a commit.
    let b = env.boot().await;
    assert_eq!(
        b.redispatch(env.job).await,
        Some(JobCommand::AllTasksSucceeded)
    );
    assert_eq!(env.runner.calls_of(Role::Implementer), 2);

    // Crash boundary 16: T2's commit exists but the table never heard of it. The commit's run
    // belongs to T2, so it is recorded rather than redone.
    env.sql(
        "UPDATE tasks SET state = 'running', commit_sha = NULL WHERE id = ?1",
        &t2.id,
    );
    env.sql("DELETE FROM commit_records WHERE task_id = ?1", &t2.id);
    assert_eq!(
        b.redispatch(env.job).await,
        Some(JobCommand::AllTasksSucceeded)
    );
    let recovered = b.tasks(env.job).await.remove(1);
    assert_eq!(
        (recovered.state, recovered.commit_sha.as_deref()),
        (TaskState::Succeeded, Some(commit.as_str()))
    );
    let event = b
        .events(env.job)
        .await
        .into_iter()
        .rfind(|e| e.kind == "task.commit_recorded")
        .expect("commit event");
    assert_eq!(event.payload["recovered"], true);
    assert_eq!(env.runner.calls_of(Role::Implementer), 2);

    // A trailer for a task the table says never ran is a disagreement, not something to adopt.
    env.sql(
        "UPDATE tasks SET state = 'ready', commit_sha = NULL WHERE id = ?1",
        &t2.id,
    );
    assert_eq!(
        b.redispatch(env.job).await,
        Some(JobCommand::Halt {
            reason: BlockReason::TaskStateMismatch
        })
    );
    let found = b
        .events(env.job)
        .await
        .into_iter()
        .rfind(|e| e.kind == "task.mismatch_detected")
        .expect("mismatch event");
    assert_eq!(found.payload["mismatches"][0]["kind"], "unrecorded_commit");
    assert_eq!(found.payload["mismatches"][0]["task"], t2.id.as_str());
    let untouched = b.tasks(env.job).await.remove(1);
    assert_eq!(
        (untouched.state, untouched.commit_sha),
        (TaskState::Ready, None),
        "nothing is fixed silently"
    );
    env.sql(
        &format!("UPDATE tasks SET state = 'succeeded', commit_sha = '{commit}' WHERE id = ?1"),
        &t2.id,
    );
    assert_eq!(env.runner.calls_of(Role::Implementer), 2);
}

#[tokio::test]
async fn a_rewritten_branch_blocks_the_job_until_a_human_repairs_it() {
    let runner = FakeRunner::new().with_script(Role::Planner, vec![Step::Output(two_task_plan())]);
    let env = Env::new(runner, settings(None, false), Profile::standard()).await;
    let b = env.boot().await;
    b.run_until(env.job, JobState::AcceptanceApproval, false)
        .await;
    let before = b.tasks(env.job).await;
    let head = env
        .repo()
        .branch_tip(BRANCH)
        .await
        .expect("tip")
        .expect("branch");

    // Someone moves the job branch back to its base: the recorded task commits are gone.
    let base = b.job(env.job).await.base_commit.expect("base");
    env.repo()
        .run(&["update-ref", &format!("refs/heads/{BRANCH}"), &base])
        .await
        .expect("rewind");
    let b = env.boot().await;
    let command = b.redispatch(env.job).await.expect("command");
    assert_eq!(
        command,
        JobCommand::Halt {
            reason: BlockReason::TaskStateMismatch
        }
    );
    let found = b
        .events(env.job)
        .await
        .into_iter()
        .rfind(|e| e.kind == "task.mismatch_detected")
        .expect("mismatch event");
    let kinds: Vec<&str> = found.payload["mismatches"]
        .as_array()
        .expect("list")
        .iter()
        .filter_map(|m| m["kind"].as_str())
        .collect();
    assert_eq!(kinds, ["commit_missing", "commit_missing"]);
    assert_eq!(
        b.tasks(env.job).await,
        before,
        "the table is left as it was"
    );

    // The mismatch blocks the job; it is recoverable once the branch is repaired.
    let blocked = b
        .dispatcher
        .engine()
        .submit(env.job, command, Actor::Scheduler)
        .await
        .expect("block");
    assert_eq!(blocked.job.state, JobState::Blocked);
    assert_eq!(
        blocked.job.blocked.as_ref().map(|b| b.reason.clone()),
        Some(BlockReason::TaskStateMismatch)
    );
    env.repo()
        .run(&["update-ref", &format!("refs/heads/{BRANCH}"), &head])
        .await
        .expect("repair");
    b.dispatcher
        .engine()
        .submit(
            env.job,
            JobCommand::ResolveBlock {
                target: JobState::Implementation,
            },
            Actor::local_user(),
        )
        .await
        .expect("resolve");
    let done = b.run_until(env.job, JobState::Done, true).await;
    assert_eq!(done.job.state, JobState::Done);
    assert_eq!(
        env.runner.calls_of(Role::Implementer),
        2,
        "resuming re-ran no task"
    );
    assert_eq!(runs_of(&env.db, "implementer"), 2);
}
