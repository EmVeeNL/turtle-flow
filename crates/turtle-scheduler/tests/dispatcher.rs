//! Dispatcher behaviour: ordering, retries, failures, holds, cancellation races, fencing.

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::*;
use turtle_domain::{Actor, GateKind, JobState};
use turtle_scheduler::{CrashPlan, ExecError};
use turtle_store::{ManualClock, Store};
use turtle_workflow::{BlockReason, JobCommand as C, PauseMode, Profile};

fn manual_store() -> (turtle_scheduler::SharedStore, Arc<ManualClock>) {
    let clock = Arc::new(ManualClock::new());
    let store = Store::open_in_memory(clock.clone()).expect("open");
    (turtle_scheduler::SharedStore::new(store), clock)
}

/// Ticks until nothing is left to do, granting the two human approvals when they are due.
async fn run_to_done(d: &TestDispatcher, job: turtle_domain::JobId) {
    for _ in 0..200 {
        match state_of(d, job).await {
            JobState::ScopeApproval => {
                d.engine()
                    .submit(
                        job,
                        C::ApprovalGranted {
                            gate: GateKind::Scope,
                        },
                        Actor::local_user(),
                    )
                    .await
                    .expect("grant");
            }
            JobState::AcceptanceApproval => {
                d.engine()
                    .submit(
                        job,
                        C::ApprovalGranted {
                            gate: GateKind::Acceptance,
                        },
                        Actor::local_user(),
                    )
                    .await
                    .expect("grant");
            }
            _ => {}
        }
        let report = d.tick().await.expect("tick");
        let unfinished = d
            .engine()
            .store()
            .with(|s| s.transaction(|tx| tx.unfinished_effect_count()))
            .await
            .expect("count");
        if state_of(d, job).await == JobState::Done && unfinished == 0 {
            return;
        }
        if report.claimed == 0 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }
    panic!("job did not finish");
}

#[tokio::test]
async fn full_job_runs_to_done_with_effects_in_order() {
    let (store, _clock) = manual_store();
    let job = create_job(&store, Profile::standard()).await;
    let world = Arc::new(World::default());
    let exec = SimExecutor::new(Arc::clone(&world));
    let (d, report) = start_dispatcher(&store, &exec, CrashPlan::never()).await;
    assert_eq!(report.effects_requeued, 0);

    d.engine()
        .submit(job, C::Start, Actor::local_user())
        .await
        .expect("start");
    run_to_done(&d, job).await;

    let order = world.order.lock().expect("lock").clone();
    let pos = |k: &str| {
        order
            .iter()
            .position(|o| o == k)
            .unwrap_or_else(|| panic!("{k} never ran: {order:?}"))
    };
    assert!(
        pos("create_worktree") < pos("dispatch_ready_tasks"),
        "{order:?}"
    );
    assert!(pos("run_gates") < pos("run_verification") && pos("run_verification") < pos("deliver"));
    assert!(pos("deliver") < pos("cleanup_worktree"));
    assert!(
        world.keys().iter().all(|k| world.executions_of(k) == 1),
        "no crash, so every effect ran once"
    );

    let events = store.with(|s| s.list_events(None)).await.expect("events");
    assert!(matches!(
        store.with(|s| s.verify_chain()).await.expect("verify"),
        turtle_store::ChainVerdict::Intact { .. }
    ));
    assert!(events.iter().all(|e| e.kind != "effect.failed"));
}

#[tokio::test]
async fn retryable_failures_back_off_and_then_succeed() {
    let (store, clock) = manual_store();
    let job = create_job(&store, Profile::standard()).await;
    let exec = SimExecutor::new(Arc::new(World::default()));
    exec.fail(
        "start_run",
        vec![
            ExecError::Retryable("503".into()),
            ExecError::Retryable("503".into()),
        ],
    );
    let (d, _) = start_dispatcher(&store, &exec, CrashPlan::never()).await;
    d.engine()
        .submit(job, C::Start, Actor::local_user())
        .await
        .expect("start");

    let r = d.tick().await.expect("tick");
    assert_eq!((r.claimed, r.requeued), (1, 1));
    assert_eq!(
        d.tick().await.expect("tick").claimed,
        0,
        "backoff has not elapsed"
    );
    clock.advance(Duration::from_secs(10));
    let r = d.tick().await.expect("tick");
    assert_eq!((r.claimed, r.requeued), (1, 1));
    assert_eq!(
        d.tick().await.expect("tick").claimed,
        0,
        "second backoff is longer"
    );
    clock.advance(Duration::from_secs(10));
    assert_eq!(
        d.tick().await.expect("tick").claimed,
        0,
        "10 s is not enough for the 20 s backoff"
    );
    clock.advance(Duration::from_secs(10));
    let r = d.tick().await.expect("tick");
    assert_eq!(r.completed, 1);
    assert_eq!(
        state_of(&d, job).await,
        JobState::ScopeApproval,
        "the result was applied once"
    );
}

#[tokio::test]
async fn fatal_failure_blocks_the_job_and_still_notifies() {
    let (store, _clock) = manual_store();
    let job = create_job(&store, Profile::standard()).await;
    let exec = SimExecutor::new(Arc::new(World::default()));
    exec.fail("start_run", vec![ExecError::Fatal("not logged in".into())]);
    let (d, _) = start_dispatcher(&store, &exec, CrashPlan::never()).await;
    d.engine()
        .submit(job, C::Start, Actor::local_user())
        .await
        .expect("start");

    let r = d.tick().await.expect("tick");
    assert_eq!(r.failed, 1);
    let rec = store.with(move |s| s.load_job(job)).await.expect("load");
    assert_eq!(rec.job.state, JobState::Blocked);
    let info = rec.job.blocked.expect("block info");
    assert_eq!(
        info.reason,
        BlockReason::Unrecoverable {
            code: "effect_failed:start_run".into()
        }
    );
    assert_eq!(info.from, JobState::Specification);

    // Control effects (release reservations, notify) still run for the blocked job.
    let r = d.tick().await.expect("tick");
    assert_eq!(r.completed, 2);
    let kinds: Vec<_> = store
        .with(move |s| s.list_effects(job))
        .await
        .expect("effects")
        .into_iter()
        .map(|e| (e.kind, e.status))
        .collect();
    assert!(
        kinds.contains(&("start_run".into(), "failed".into())),
        "{kinds:?}"
    );
}

#[tokio::test]
async fn exhausting_retries_blocks_the_job() {
    let (store, clock) = manual_store();
    let job = create_job(&store, Profile::standard()).await;
    let exec = SimExecutor::new(Arc::new(World::default()));
    exec.fail(
        "start_run",
        (0..10)
            .map(|_| ExecError::Retryable("503".into()))
            .collect(),
    );
    let (d, _) = start_dispatcher(&store, &exec, CrashPlan::never()).await;
    d.engine()
        .submit(job, C::Start, Actor::local_user())
        .await
        .expect("start");

    let mut failed = 0;
    for _ in 0..6 {
        let r = d.tick().await.expect("tick");
        failed += r.failed;
        clock.advance(Duration::from_secs(130));
    }
    assert_eq!(failed, 1, "exactly one permanent failure after 5 attempts");
    assert_eq!(state_of(&d, job).await, JobState::Blocked);
}

#[tokio::test]
async fn paused_job_does_not_run_and_resume_continues() {
    let (store, _clock) = manual_store();
    let job = create_job(&store, Profile::standard()).await;
    let exec = SimExecutor::new(Arc::new(World::default()));
    let (d, _) = start_dispatcher(&store, &exec, CrashPlan::never()).await;
    d.engine()
        .submit(job, C::Start, Actor::local_user())
        .await
        .expect("start");
    d.engine()
        .submit(
            job,
            C::Pause {
                mode: PauseMode::Soft,
            },
            Actor::local_user(),
        )
        .await
        .expect("pause");

    assert_eq!(d.tick().await.expect("tick").claimed, 0);
    assert_eq!(state_of(&d, job).await, JobState::Specification);
    d.engine()
        .submit(job, C::Resume, Actor::local_user())
        .await
        .expect("resume");
    assert_eq!(d.tick().await.expect("tick").completed, 1);
    assert_eq!(state_of(&d, job).await, JobState::ScopeApproval);
}

#[tokio::test]
async fn a_result_arriving_after_cancellation_is_dropped_not_applied() {
    let (store, _clock) = manual_store();
    let job = create_job(&store, Profile::standard()).await;
    let exec = SimExecutor::new(Arc::new(World::default()));
    let gate = Gate {
        started: Arc::new(tokio::sync::Notify::new()),
        release: Arc::new(tokio::sync::Notify::new()),
    };
    *exec.gate.lock().expect("lock") = Some(gate.clone());
    let (d, _) = start_dispatcher(&store, &exec, CrashPlan::never()).await;
    d.engine()
        .submit(job, C::Start, Actor::local_user())
        .await
        .expect("start");

    let ticking = tokio::spawn({
        let d = d.clone();
        async move { d.tick().await }
    });
    gate.started.notified().await;
    // The run is in flight (and heartbeating) while the user cancels.
    tokio::time::sleep(Duration::from_millis(80)).await;
    d.engine()
        .submit(job, C::Cancel, Actor::local_user())
        .await
        .expect("cancel");
    gate.release.notify_one();

    let r = ticking.await.expect("join").expect("tick");
    assert_eq!(
        r.completed, 1,
        "the effect itself completes; the stale command is dropped"
    );
    assert_eq!(
        state_of(&d, job).await,
        JobState::Cancelled,
        "SpecProduced was not applied"
    );
    let start = store
        .with(move |s| s.list_effects(job))
        .await
        .expect("effects")
        .into_iter()
        .find(|e| e.kind == "start_run")
        .expect("effect");
    assert_eq!(start.status, "completed");
}

#[tokio::test]
async fn a_restarted_instance_fences_off_the_old_one() {
    let (store, _clock) = manual_store();
    let job = create_job(&store, Profile::standard()).await;
    let world = Arc::new(World::default());
    let exec = SimExecutor::new(Arc::clone(&world));
    let gate = Gate {
        started: Arc::new(tokio::sync::Notify::new()),
        release: Arc::new(tokio::sync::Notify::new()),
    };
    *exec.gate.lock().expect("lock") = Some(gate.clone());
    let (old, _) = start_dispatcher(&store, &exec, CrashPlan::never()).await;
    old.engine()
        .submit(job, C::Start, Actor::local_user())
        .await
        .expect("start");

    let ticking = tokio::spawn({
        let d = old.clone();
        async move { d.tick().await }
    });
    gate.started.notified().await;

    // A new daemon instance starts while the old one is still "running" (a hung process).
    *exec.gate.lock().expect("lock") = None;
    let (new, report) = start_dispatcher(&store, &exec, CrashPlan::never()).await;
    assert_eq!(
        report.effects_requeued, 1,
        "recovery took the effect away from the old instance"
    );

    // The old instance finishes later: its result must be discarded.
    gate.release.notify_one();
    let r = ticking.await.expect("join").expect("tick");
    assert_eq!((r.completed, r.lease_lost), (0, 1));
    assert_eq!(
        state_of(&old, job).await,
        JobState::Specification,
        "nothing applied by the old instance"
    );

    // The new instance runs it and applies the result exactly once.
    assert_eq!(new.tick().await.expect("tick").completed, 1);
    assert_eq!(state_of(&new, job).await, JobState::ScopeApproval);
    let key = store
        .with(move |s| s.list_effects(job))
        .await
        .expect("effects")
        .into_iter()
        .find(|e| e.kind == "start_run")
        .expect("effect")
        .idempotency_key;
    assert_eq!(
        world.executions_of(&key),
        2,
        "at-least-once: both instances ran it; the executor must be idempotent"
    );
}

#[tokio::test]
async fn executor_panic_is_contained_and_retried() {
    let (store, clock) = manual_store();
    let job = create_job(&store, Profile::standard()).await;
    let exec = SimExecutor::new(Arc::new(World::default()));
    exec.faults
        .lock()
        .expect("lock")
        .panic_once
        .push("start_run".into());
    let (d, _) = start_dispatcher(&store, &exec, CrashPlan::never()).await;
    d.engine()
        .submit(job, C::Start, Actor::local_user())
        .await
        .expect("start");

    let r = d
        .tick()
        .await
        .expect("the dispatcher survives an executor panic");
    assert_eq!(r.requeued, 1);
    clock.advance(Duration::from_secs(10));
    assert_eq!(d.tick().await.expect("tick").completed, 1);
    assert_eq!(state_of(&d, job).await, JobState::ScopeApproval);
}

#[tokio::test]
async fn concurrent_limit_is_respected_across_jobs() {
    let (store, _clock) = manual_store();
    let exec = SimExecutor::new(Arc::new(World::default()));
    let (d, _) = start_dispatcher(&store, &exec, CrashPlan::never()).await;
    let mut jobs = Vec::new();
    for i in 0..5 {
        // Distinct projects so job keys do not collide.
        let id = store
            .with(move |s| {
                s.transaction(|tx| {
                    let project_id = tx.insert_project(&turtle_store::NewProject {
                        slug: format!("proj-{i}"),
                        key_prefix: ["AAA", "BBB", "CCC", "DDD", "EEE"][i].into(),
                        name: "P".into(),
                        description: String::new(),
                        repo_path: format!("/tmp/p{i}"),
                        default_branch: "main".into(),
                        default_profile: "standard".into(),
                    })?;
                    Ok(tx
                        .create_job(
                            &turtle_store::NewJob {
                                project_id,
                                title: "t".into(),
                                description: String::new(),
                                job_type: turtle_domain::JobType::Feature,
                                priority: turtle_domain::Priority::Normal,
                                profile: Profile::standard(),
                            },
                            &Actor::local_user(),
                        )?
                        .job
                        .id)
                })
            })
            .await
            .expect("job");
        d.engine()
            .submit(id, C::Start, Actor::local_user())
            .await
            .expect("start");
        jobs.push(id);
    }
    let first = d.tick().await.expect("tick");
    assert_eq!(
        first.claimed, 3,
        "five jobs are ready but max_concurrent is 3"
    );

    let mut ticks = 1;
    loop {
        let mut all_there = true;
        for job in &jobs {
            all_there &= state_of(&d, *job).await == JobState::ScopeApproval;
        }
        if all_there {
            break;
        }
        let r = d.tick().await.expect("tick");
        assert!(r.claimed <= 3, "limit exceeded: {r:?}");
        ticks += 1;
        assert!(ticks < 20, "jobs are not progressing");
    }
    assert!(ticks >= 2, "five jobs cannot finish in one batch of three");
}

#[tokio::test]
async fn run_loop_processes_work_and_marks_the_instance_stopped_on_shutdown() {
    let (store, _clock) = manual_store();
    let job = create_job(&store, Profile::standard()).await;
    let exec = SimExecutor::new(Arc::new(World::default()));
    let (d, _) = start_dispatcher(&store, &exec, CrashPlan::never()).await;
    let (tx, rx) = tokio::sync::watch::channel(false);
    let running = tokio::spawn({
        let d = d.clone();
        async move { d.run(rx).await }
    });

    d.engine()
        .submit(job, C::Start, Actor::local_user())
        .await
        .expect("start");
    d.wake();
    for _ in 0..200 {
        if state_of(&d, job).await == JobState::ScopeApproval {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(state_of(&d, job).await, JobState::ScopeApproval);

    tx.send(true).expect("signal");
    running.await.expect("join").expect("run loop ends cleanly");
    let id = d.instance_id().to_owned();
    let stopped: Option<String> = store
        .with(move |s| {
            s.raw_connection()
                .query_row(
                    "SELECT stopped_at FROM daemon_instances WHERE instance_id = ?1",
                    [id],
                    |r| r.get(0),
                )
                .expect("row")
        })
        .await;
    assert!(stopped.is_some(), "clean shutdown is recorded");
}
