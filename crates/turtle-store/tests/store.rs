//! Integration tests for the store: atomic decisions, optimistic concurrency, hash-chain
//! tamper detection, and rebuilding job state from the event log (NFR-017).

use std::sync::Arc;

use turtle_domain::{Actor, ApprovalValidity, GateKind, JobState, JobType, Priority};
use turtle_store::{ChainVerdict, JobRecord, NewJob, NewProject, StepClock, Store, StoreError};
use turtle_workflow::{DecisionContext, JobCommand as C, PreflightDecision, Profile, decide};

fn store() -> Store {
    Store::open_in_memory(Arc::new(StepClock::default())).expect("open")
}

fn project() -> NewProject {
    NewProject {
        slug: "payments-api".into(),
        key_prefix: "API".into(),
        name: "Payments API".into(),
        description: String::new(),
        repo_path: "/tmp/payments-api".into(),
        default_branch: "main".into(),
        default_profile: "standard".into(),
    }
}

fn seed(store: &mut Store) -> JobRecord {
    store
        .transaction(|tx| {
            let project_id = tx.insert_project(&project())?;
            tx.create_job(
                &NewJob {
                    project_id,
                    title: "Add rate limiting".into(),
                    description: "details".into(),
                    job_type: JobType::Feature,
                    priority: Priority::High,
                    profile: Profile::standard(),
                },
                &Actor::local_user(),
            )
        })
        .expect("seed")
}

fn cx() -> DecisionContext {
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

/// Decide and persist one command; returns the stored record afterwards.
fn run(
    store: &mut Store,
    rec: &JobRecord,
    cmd: &C,
    actor: &Actor,
) -> Result<JobRecord, StoreError> {
    let d = decide(&rec.job, cmd, actor, &cx()).expect("decision accepted");
    store.transaction(|tx| {
        tx.apply_decision(rec.job.version, &d, cmd, actor)?;
        tx.load_job(rec.job.id)
    })
}

#[test]
fn creates_job_with_key_and_round_trips() {
    let mut s = store();
    let rec = seed(&mut s);
    assert_eq!(rec.key.to_string(), "API-1");
    assert_eq!(rec.job.state, JobState::Backlog);
    assert_eq!(s.load_job(rec.job.id).expect("load"), rec);

    let second = s
        .transaction(|tx| {
            tx.create_job(
                &NewJob {
                    project_id: rec.project_id,
                    title: "Second".into(),
                    description: String::new(),
                    job_type: JobType::Bugfix,
                    priority: Priority::Low,
                    profile: Profile::fast(),
                },
                &Actor::local_user(),
            )
        })
        .expect("second job");
    assert_eq!(second.key.to_string(), "API-2");
}

#[test]
fn decision_persists_state_events_and_effects_together() {
    let mut s = store();
    let rec = seed(&mut s);
    let after = run(&mut s, &rec, &C::Start, &Actor::local_user()).expect("start");
    assert_eq!(after.job.state, JobState::Specification);
    assert_eq!(after.job.version, 1);

    let kinds: Vec<_> = s
        .list_events(Some(rec.job.id))
        .expect("events")
        .into_iter()
        .map(|e| e.kind)
        .collect();
    assert_eq!(kinds, ["job.created", "job.command", "job.stage_entered"]);

    let effects = s.list_effects(rec.job.id).expect("effects");
    assert_eq!(effects.len(), 1);
    assert_eq!(effects[0].kind, "start_run");
    assert_eq!(effects[0].status, "pending");
    assert_eq!(
        effects[0].idempotency_key,
        format!("job:{}:v1:e0", rec.job.id)
    );
}

#[test]
fn concurrent_decision_on_stale_version_conflicts_and_writes_nothing() {
    let mut s = store();
    let rec = seed(&mut s);
    let actor = Actor::local_user();
    // Two writers read version 0 and both decide.
    let d1 = decide(&rec.job, &C::Start, &actor, &cx()).expect("d1");
    let d2 = decide(&rec.job, &C::Cancel, &actor, &cx()).expect("d2");
    s.transaction(|tx| tx.apply_decision(0, &d1, &C::Start, &actor))
        .expect("first writer wins");
    let events_before = s.list_events(None).expect("events").len();

    let err = s
        .transaction(|tx| tx.apply_decision(0, &d2, &C::Cancel, &actor))
        .expect_err("second writer loses");
    assert!(matches!(
        err,
        StoreError::VersionConflict { expected: 0, .. }
    ));
    assert_eq!(s.list_events(None).expect("events").len(), events_before);
    assert_eq!(
        s.load_job(rec.job.id).expect("load").job.state,
        JobState::Specification
    );
    assert_eq!(s.list_effects(rec.job.id).expect("effects").len(), 1);
}

#[test]
fn failure_after_state_and_events_were_written_rolls_everything_back() {
    let mut s = store();
    let rec = seed(&mut s);
    let actor = Actor::local_user();
    let d = decide(&rec.job, &C::Start, &actor, &cx()).expect("decision");
    let events_before = s.list_events(None).expect("events").len();

    // Plant an effect with the key the decision is about to use. `apply_decision` updates the
    // job and appends events first, then fails on the unique idempotency key while enqueuing.
    s.raw_connection()
        .execute(
            "INSERT INTO effects(id, job_id, kind, idempotency_key, payload, status, attempts,
                                 max_attempts, created_event_seq, created_at)
             VALUES ('01JB7Y0Q4N3T9Z8K2M6W5R1C7D', ?1, 'notify_user', ?2, '{}', 'pending', 0, 5, 1,
                     '2026-10-24T00:00:00.000Z')",
            rusqlite_params(rec.job.id),
        )
        .expect("plant conflicting effect");

    let err = s
        .transaction(|tx| tx.apply_decision(0, &d, &C::Start, &actor))
        .expect_err("enqueue must fail");
    assert!(
        matches!(err, StoreError::Sqlite(_)),
        "unexpected error: {err}"
    );

    assert_eq!(
        s.list_events(None).expect("events").len(),
        events_before,
        "events rolled back"
    );
    let after = s.load_job(rec.job.id).expect("load");
    assert_eq!(after, rec, "job state and version rolled back");
    assert_eq!(
        s.list_effects(rec.job.id).expect("effects").len(),
        1,
        "only the planted effect"
    );
    assert!(matches!(
        s.verify_chain().expect("verify"),
        ChainVerdict::Intact { .. }
    ));
}

fn rusqlite_params(job: turtle_domain::JobId) -> [String; 2] {
    [job.to_string(), format!("job:{job}:v1:e0")]
}

#[test]
fn hash_chain_verifies_and_detects_tampering() {
    let mut s = store();
    let rec = seed(&mut s);
    let rec = run(&mut s, &rec, &C::Start, &Actor::local_user()).expect("start");
    run(
        &mut s,
        &rec,
        &C::Pause {
            mode: turtle_workflow::PauseMode::Soft,
        },
        &Actor::local_user(),
    )
    .expect("pause");

    let ChainVerdict::Intact { events, head } = s.verify_chain().expect("verify") else {
        panic!("chain should be intact")
    };
    assert_eq!(events, s.list_events(None).expect("events").len() as u64);
    assert_eq!(head.len(), 64);

    // Edit an old event's payload (bypassing the append-only triggers like an attacker with
    // file access could, by dropping them first).
    let conn = s.raw_connection();
    conn.execute_batch("DROP TRIGGER trg_events_no_update;")
        .expect("drop trigger");
    conn.execute(
        "UPDATE events SET payload = '{\"command\":\"forged\"}' WHERE type = 'job.command' AND seq = (SELECT min(seq) FROM events WHERE type='job.command')",
        [],
    )
    .expect("tamper");
    let ChainVerdict::Broken { seq, reason } = s.verify_chain().expect("verify") else {
        panic!("tampering must be detected")
    };
    assert!(seq > 0 && reason.contains("hash"), "{reason}");
}

#[test]
fn deleting_an_event_breaks_the_chain() {
    let mut s = store();
    let rec = seed(&mut s);
    run(&mut s, &rec, &C::Start, &Actor::local_user()).expect("start");
    let conn = s.raw_connection();
    conn.execute_batch("DROP TRIGGER trg_events_no_delete;")
        .expect("drop trigger");
    conn.execute("DELETE FROM events WHERE seq = 2", [])
        .expect("delete");
    assert!(matches!(
        s.verify_chain().expect("verify"),
        ChainVerdict::Broken { .. }
    ));
}

#[test]
fn append_only_triggers_block_updates_and_deletes() {
    let mut s = store();
    seed(&mut s);
    let conn = s.raw_connection();
    assert!(conn.execute("UPDATE events SET type = 'x'", []).is_err());
    assert!(conn.execute("DELETE FROM events", []).is_err());
}

#[test]
fn job_state_can_be_rebuilt_from_events() {
    use turtle_workflow::{BlockReason, ReviewVerdict};
    let mut s = store();
    let mut rec = seed(&mut s);
    let u = Actor::local_user();
    let m = Actor::Scheduler;
    let script: Vec<(C, &Actor)> = vec![
        (C::Start, &u),
        (
            C::SpecProduced {
                valid: true,
                criteria_count: 3,
            },
            &m,
        ),
        (
            C::ApprovalGranted {
                gate: GateKind::Scope,
            },
            &u,
        ),
        (
            C::PlanProduced {
                valid: true,
                within_scope: true,
            },
            &m,
        ),
        (
            C::Pause {
                mode: turtle_workflow::PauseMode::Soft,
            },
            &u,
        ),
        (
            C::RateLimited {
                provider: "codex".into(),
            },
            &m,
        ),
        (C::Resume, &u),
        (
            C::RateLimitCleared {
                provider: "codex".into(),
            },
            &m,
        ),
        (C::AllTasksSucceeded, &m),
        (
            C::GatesFinished {
                all_required_passed: true,
            },
            &m,
        ),
        (
            C::ReviewCompleted {
                verdict: ReviewVerdict::Fixable,
            },
            &m,
        ),
        (
            C::Halt {
                reason: BlockReason::Timeout,
            },
            &m,
        ),
        (
            C::ResolveBlock {
                target: JobState::Implementation,
            },
            &u,
        ),
        (C::BudgetExhausted, &m),
        (C::Cancel, &u),
    ];
    for (cmd, actor) in &script {
        rec = run(&mut s, &rec, cmd, actor).unwrap_or_else(|e| panic!("{cmd:?}: {e}"));
        let rebuilt = s.rebuild_job(rec.job.id).expect("rebuild");
        assert_eq!(rebuilt.state, rec.job.state, "after {cmd:?}");
        assert_eq!(rebuilt.holds, rec.job.holds, "after {cmd:?}");
        assert_eq!(rebuilt.blocked, rec.job.blocked, "after {cmd:?}");
    }
    assert_eq!(rec.job.state, JobState::Cancelled);
    assert!(rec.job.holds.is_empty());
}

#[test]
fn database_file_is_private_and_reopens_with_same_identity() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("data").join("turtle.db");
    let id = {
        let s = Store::open(&path, Arc::new(StepClock::default())).expect("open");
        s.installation_id().expect("id")
    };
    let mode =
        |p: &std::path::Path| std::fs::metadata(p).expect("meta").permissions().mode() & 0o777;
    assert_eq!(mode(&path), 0o600);
    assert_eq!(mode(path.parent().expect("parent")), 0o700);

    let s = Store::open(&path, Arc::new(StepClock::default())).expect("reopen");
    assert_eq!(s.installation_id().expect("id"), id);
}

#[test]
fn projects_and_jobs_can_be_looked_up_by_slug_and_key() {
    let mut store = store();
    let first = seed(&mut store);
    let second = store
        .transaction(|tx| {
            tx.create_job(
                &NewJob {
                    project_id: first.project_id,
                    title: "Second".into(),
                    description: String::new(),
                    job_type: JobType::Bugfix,
                    priority: Priority::Normal,
                    profile: Profile::fast(),
                },
                &Actor::local_user(),
            )
        })
        .expect("second job");

    let projects = store.list_projects().expect("list");
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].default_profile, "standard");
    assert_eq!(
        store.project_by_slug("payments-api").expect("slug").id,
        first.project_id
    );
    assert!(matches!(
        store.project_by_slug("nope"),
        Err(StoreError::NotFound(_))
    ));

    assert_eq!(
        store
            .find_job(&"API-1".parse().expect("key"))
            .expect("find")
            .job
            .id,
        first.job.id
    );
    assert_eq!(
        store
            .find_job(&"API-2".parse().expect("key"))
            .expect("find")
            .job
            .id,
        second.job.id
    );
    assert!(matches!(
        store.find_job(&"API-3".parse().expect("key")),
        Err(StoreError::NotFound(_))
    ));

    let newest_first: Vec<_> = store
        .list_jobs(Some(first.project_id))
        .expect("jobs")
        .into_iter()
        .map(|j| j.job.id)
        .collect();
    assert_eq!(newest_first, vec![second.job.id, first.job.id]);
    assert_eq!(store.list_jobs(None).expect("all").len(), 2);
}
