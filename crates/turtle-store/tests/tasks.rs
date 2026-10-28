//! Plan tasks at the store level: persistence, the state machine, and the links from runs,
//! gate results, usage and commits to their task.

use std::sync::Arc;

use serde_json::json;
use turtle_domain::{Actor, ArtifactKind, JobType, Priority, Role, RunId, Sha256, UsageValue};
use turtle_store::{
    ChainVerdict, JobRecord, NewGateResult, NewJob, NewProject, NewRevision, NewRun, NewTask,
    RunEnd, StepClock, Store, StoreError, TaskState, TrailerCommit, task_id,
};
use turtle_workflow::Profile;

fn setup() -> (Store, JobRecord) {
    let mut store = Store::open_in_memory(Arc::new(StepClock::default())).expect("open");
    let rec = store
        .transaction(|tx| {
            let project_id = tx.insert_project(&NewProject {
                slug: "proj".into(),
                key_prefix: "API".into(),
                name: "P".into(),
                description: String::new(),
                repo_path: "/tmp/p".into(),
                default_branch: "main".into(),
                default_profile: "standard".into(),
            })?;
            tx.create_job(
                &NewJob {
                    project_id,
                    title: "t".into(),
                    description: String::new(),
                    job_type: JobType::Feature,
                    priority: Priority::Normal,
                    profile: Profile::standard(),
                },
                &Actor::local_user(),
            )
        })
        .expect("seed");
    (store, rec)
}

fn plan_revision(store: &mut Store, job: &JobRecord, byte: u8) -> String {
    store
        .transaction(|tx| {
            tx.add_revision(&NewRevision {
                job_id: job.job.id,
                kind: ArtifactKind::Plan,
                sha256: Sha256::from_bytes([byte; 32]),
                size_bytes: 10,
                media_type: "application/json".into(),
                schema_id: None,
                author: Actor::Scheduler,
            })
        })
        .expect("plan revision")
        .id
}

fn task(id: &str, deps: &[&str]) -> NewTask {
    NewTask {
        plan_task_id: id.into(),
        title: format!("Task {id}"),
        instructions: format!("Do {id}"),
        expected_paths: vec!["src/**".into()],
        criteria_refs: vec!["AC-1".into()],
        depends_on: deps.iter().map(|d| (*d).to_owned()).collect(),
        backfill: None,
    }
}

fn begin_run(store: &mut Store, job: &JobRecord, role: Role) -> RunId {
    let run = RunId::new();
    store
        .transaction(|tx| {
            tx.begin_run(&NewRun {
                id: run,
                job_id: job.job.id,
                role,
                stage: "implementation".into(),
                provider: "fake".into(),
                model: "m".into(),
                adapter_version: "test".into(),
                context_manifest_rev: None,
            })
        })
        .expect("run");
    run
}

fn event_kinds(store: &Store, job: &JobRecord) -> Vec<String> {
    store
        .list_events(Some(job.job.id))
        .expect("events")
        .into_iter()
        .map(|e| e.kind)
        .collect()
}

#[test]
fn plan_tasks_round_trip_once_with_dependencies_and_events() {
    let (mut store, job) = setup();
    let rev = plan_revision(&mut store, &job, 1);
    let tasks = [task("T1", &[]), task("T2", &["T1"])];
    let stored = store
        .transaction(|tx| tx.plan_tasks(job.job.id, &rev, &tasks))
        .expect("persist");
    assert_eq!(stored.len(), 2);
    assert_eq!(stored[0].id, task_id(&rev, "T1"));
    assert_eq!(stored[0].id, format!("{rev}/T1"));
    assert_eq!(
        (stored[0].plan_task_id.as_str(), stored[0].ordinal),
        ("T1", 1)
    );
    assert_eq!(stored[0].state, TaskState::Ready);
    assert_eq!(stored[1].state, TaskState::Pending, "waits for T1");
    assert_eq!(stored[1].depends_on, [task_id(&rev, "T1")]);
    assert_eq!(stored[1].instructions, "Do T2");

    // A second call for the same revision changes nothing, even after progress.
    store
        .transaction(|tx| tx.transition_task(&stored[0].id, TaskState::Running, None, json!({})))
        .expect("running");
    let again = store
        .transaction(|tx| tx.plan_tasks(job.job.id, &rev, &tasks))
        .expect("idempotent");
    assert_eq!(again[0].state, TaskState::Running);
    assert_eq!(store.tasks_for_job(job.job.id).expect("read").len(), 2);

    let created = event_kinds(&store, &job)
        .iter()
        .filter(|k| *k == "task.created")
        .count();
    assert_eq!(created, 2);
    let events = store.list_events(Some(job.job.id)).expect("events");
    let row: Option<String> = store
        .raw_connection()
        .query_row(
            "SELECT task_id FROM events WHERE seq = ?1",
            [events.last().expect("event").seq],
            |r| r.get(0),
        )
        .expect("task id column");
    assert_eq!(row.as_deref(), Some(stored[0].id.as_str()));
    assert!(matches!(
        store.verify_chain().expect("verify"),
        ChainVerdict::Intact { .. }
    ));
}

#[test]
fn task_state_machine_allows_only_documented_moves() {
    use TaskState as S;
    let all = [
        S::Pending,
        S::Ready,
        S::Running,
        S::Gating,
        S::Fixing,
        S::Succeeded,
        S::Failed,
        S::Skipped,
        S::Superseded,
    ];
    for terminal in [S::Succeeded, S::Skipped, S::Superseded] {
        assert!(
            all.iter().all(|to| !terminal.can_become(*to)),
            "{terminal} is final"
        );
    }
    assert!(S::Ready.can_become(S::Running));
    assert!(S::Running.can_become(S::Gating));
    assert!(S::Gating.can_become(S::Fixing));
    assert!(S::Fixing.can_become(S::Gating));
    assert!(S::Gating.can_become(S::Succeeded));
    assert!(S::Fixing.can_become(S::Failed));
    assert!(S::Failed.can_become(S::Ready));
    assert!(S::Gating.can_become(S::Running), "retry after a restart");
    assert!(!S::Pending.can_become(S::Running), "dependencies first");
    assert!(!S::Fixing.can_become(S::Succeeded), "fixes are gated");
    assert!(!S::Failed.can_become(S::Running));

    let (mut store, job) = setup();
    let rev = plan_revision(&mut store, &job, 1);
    let stored = store
        .transaction(|tx| tx.plan_tasks(job.job.id, &rev, &[task("T1", &[]), task("T2", &["T1"])]))
        .expect("persist");
    let err = store
        .transaction(|tx| tx.transition_task(&stored[1].id, S::Running, None, json!({})))
        .expect_err("pending cannot run");
    assert!(
        matches!(err, StoreError::InvalidTaskTransition { .. }),
        "{err}"
    );
    let t1 = &stored[0].id;
    for to in [S::Running, S::Gating, S::Fixing, S::Gating, S::Fixing] {
        store
            .transaction(|tx| tx.transition_task(t1, to, None, json!({})))
            .expect("move");
    }
    let fixing = store.task(t1).expect("task");
    assert_eq!((fixing.state, fixing.fix_iterations), (S::Fixing, 2));
    store
        .transaction(|tx| tx.transition_task(t1, S::Failed, None, json!({"reason": "limit"})))
        .expect("failed");
    store
        .transaction(|tx| tx.transition_task(t1, S::Ready, None, json!({})))
        .expect("resumed");
    let event = store
        .list_events(Some(job.job.id))
        .expect("events")
        .into_iter()
        .rfind(|e| e.kind == "task.state_changed")
        .expect("state event");
    assert_eq!(event.payload["from"], "failed");
    assert_eq!(event.payload["to"], "ready");
}

#[test]
fn runs_gates_usage_and_commits_link_to_their_task() {
    let (mut store, job) = setup();
    let rev = plan_revision(&mut store, &job, 1);
    let stored = store
        .transaction(|tx| tx.plan_tasks(job.job.id, &rev, &[task("T1", &[]), task("T2", &["T1"])]))
        .expect("persist");
    let (t1, t2) = (stored[0].id.clone(), stored[1].id.clone());

    // A stage-level run before any task is active belongs to no task.
    let review = begin_run(&mut store, &job, Role::Reviewer);
    store
        .transaction(|tx| tx.transition_task(&t1, TaskState::Running, None, json!({})))
        .expect("running");
    let run = begin_run(&mut store, &job, Role::Implementer);
    let other = begin_run(&mut store, &job, Role::Reviewer);
    store
        .transaction(|tx| {
            tx.finish_run(run, RunEnd::Succeeded, None, UsageValue::Unavailable)?;
            tx.transition_task(&t1, TaskState::Gating, Some(run), json!({}))?;
            for (gate, outcome) in [("build", "passed"), ("test", "failed"), ("test", "passed")] {
                tx.record_task_gate(&NewGateResult {
                    job_id: job.job.id,
                    task_id: &t1,
                    gate,
                    outcome,
                    command: Some("true"),
                    exit_code: Some(0),
                    duration_ms: Some(5),
                    head_commit: "base",
                    tree_hash: "tree",
                    summary: &json!({"fix_iteration": 0}),
                })?;
            }
            tx.record_task_gate(&NewGateResult {
                job_id: job.job.id,
                task_id: &t1,
                gate: "lint",
                outcome: "timeout",
                command: Some("sleep 9"),
                exit_code: None,
                duration_ms: None,
                head_commit: "base",
                tree_hash: "tree",
                summary: &json!({}),
            })
        })
        .expect("gating");
    assert_eq!(
        store.run_by_id(&review.to_string()).expect("run").role,
        "reviewer"
    );
    let conn = store.raw_connection();
    let link = |id: RunId| -> Option<String> {
        conn.query_row(
            "SELECT task_id FROM agent_runs WHERE id = ?1",
            [id.to_string()],
            |r| r.get(0),
        )
        .expect("run row")
    };
    assert_eq!(link(review), None);
    assert_eq!(link(other), None, "only code roles belong to a task");
    assert_eq!(link(run).as_deref(), Some(t1.as_str()));
    let usage_task: Option<String> = conn
        .query_row(
            "SELECT task_id FROM usage_records WHERE run_id = ?1",
            [run.to_string()],
            |r| r.get(0),
        )
        .expect("usage");
    assert_eq!(usage_task.as_deref(), Some(t1.as_str()));
    let stored_outcome: String = conn
        .query_row(
            "SELECT outcome FROM gate_results WHERE gate = 'lint'",
            [],
            |r| r.get(0),
        )
        .expect("lint");
    assert_eq!(stored_outcome, "error", "timeouts are stored as errors");

    let done = store
        .transaction(|tx| tx.record_task_commit(&t1, Some("abc123"), run, false))
        .expect("commit");
    assert_eq!(
        (done.state, done.commit_sha.as_deref()),
        (TaskState::Succeeded, Some("abc123"))
    );
    let (kind, task, linked_run): (String, String, String) = store
        .raw_connection()
        .query_row(
            "SELECT kind, task_id, run_id FROM commit_records WHERE sha = 'abc123'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .expect("commit record");
    assert_eq!(
        (kind.as_str(), task.as_str(), linked_run),
        ("task", t1.as_str(), run.to_string())
    );
    assert_eq!(store.task(&t2).expect("t2").state, TaskState::Ready);

    let summaries = store.task_summaries(Some(job.job.id)).expect("summaries");
    assert_eq!(summaries[0].runs, 1);
    assert_eq!(
        summaries[0].gates,
        [
            ("build".to_owned(), "passed".to_owned()),
            ("test".to_owned(), "passed".to_owned()),
            ("lint".to_owned(), "error".to_owned())
        ],
        "the latest outcome of each gate, in first-run order"
    );
    assert_eq!(
        (summaries[0].gates_passed(), summaries[0].gates_failed()),
        (2, 1)
    );
    assert_eq!((summaries[1].runs, summaries[1].gates.len()), (0, 0));
    let kinds = event_kinds(&store, &job);
    assert!(
        kinds.contains(&"task.commit_recorded".to_owned()),
        "{kinds:?}"
    );
    assert!(matches!(
        store.verify_chain().expect("verify"),
        ChainVerdict::Intact { .. }
    ));
}

#[test]
fn a_new_plan_revision_supersedes_unfinished_tasks_and_keeps_succeeded_ones() {
    let (mut store, job) = setup();
    let first = plan_revision(&mut store, &job, 1);
    let old = store
        .transaction(|tx| {
            tx.plan_tasks(job.job.id, &first, &[task("T1", &[]), task("T2", &["T1"])])
        })
        .expect("first plan");
    let run = begin_run(&mut store, &job, Role::Implementer);
    store
        .transaction(|tx| {
            tx.transition_task(&old[0].id, TaskState::Running, Some(run), json!({}))?;
            tx.record_task_commit(&old[0].id, None, run, false)
        })
        .expect("t1 done without changes");
    let second = plan_revision(&mut store, &job, 2);
    let new = store
        .transaction(|tx| tx.plan_tasks(job.job.id, &second, &[task("T1", &[])]))
        .expect("second plan");
    assert_eq!(new[0].plan_revision_id, second);
    let old_t1 = store.task(&old[0].id).expect("old t1");
    assert_eq!(
        (old_t1.state, old_t1.commit_sha),
        (TaskState::Succeeded, None)
    );
    assert_eq!(
        store.task(&old[1].id).expect("old t2").state,
        TaskState::Superseded
    );
    assert_eq!(store.tasks_for_job(job.job.id).expect("all").len(), 3);
}

#[test]
fn backfilled_tasks_start_succeeded_with_their_trailer_commit() {
    let (mut store, job) = setup();
    let rev = plan_revision(&mut store, &job, 1);
    let mut first = task("T1", &[]);
    first.backfill = Some(TrailerCommit {
        sha: "feed".into(),
        run_id: Some(RunId::new().to_string()),
    });
    let stored = store
        .transaction(|tx| tx.plan_tasks(job.job.id, &rev, &[first, task("T2", &["T1"])]))
        .expect("backfill");
    assert_eq!(
        (stored[0].state, stored[0].commit_sha.as_deref()),
        (TaskState::Succeeded, Some("feed"))
    );
    assert_eq!(stored[1].state, TaskState::Ready);
    let run: Option<String> = store
        .raw_connection()
        .query_row(
            "SELECT run_id FROM commit_records WHERE sha = 'feed'",
            [],
            |r| r.get(0),
        )
        .expect("record");
    assert_eq!(run, None, "an unknown run is not linked");
    let created = store
        .list_events(Some(job.job.id))
        .expect("events")
        .into_iter()
        .find(|e| e.kind == "task.created")
        .expect("created");
    assert_eq!(created.payload["source"], "trailer_backfill");
}
