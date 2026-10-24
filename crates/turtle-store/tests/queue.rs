//! Effect queue rules: ordering, holds, control effects, leases, backoff and recovery.

use std::sync::Arc;
use std::time::Duration;

use turtle_domain::{Actor, ApprovalValidity, GateKind, JobType, Priority};
use turtle_store::{
    ClaimParams, EffectFate, JobRecord, ManualClock, NewJob, NewProject, Store, StoreError,
};
use turtle_workflow::{
    BlockReason, DecisionContext, Effect, JobCommand as C, PauseMode, PreflightDecision, Profile,
    decide,
};

const TTL: Duration = Duration::from_secs(30);

fn setup() -> (Store, Arc<ManualClock>, JobRecord, String) {
    let clock = Arc::new(ManualClock::new());
    let mut store = Store::open_in_memory(clock.clone()).expect("open");
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
    let instance = store
        .transaction(|tx| tx.register_instance("test", 1))
        .expect("instance");
    (store, clock, rec, instance)
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

fn submit(store: &mut Store, job: &JobRecord, cmd: &C) -> JobRecord {
    let actor = if matches!(
        cmd,
        C::Start | C::ApprovalGranted { .. } | C::Cancel | C::Pause { .. } | C::Resume
    ) {
        Actor::local_user()
    } else {
        Actor::Scheduler
    };
    let d = decide(&job.job, cmd, &actor, &cx()).expect("accepted");
    store
        .transaction(|tx| {
            tx.apply_decision(job.job.version, &d, cmd, &actor)?;
            tx.load_job(job.job.id)
        })
        .expect("persist")
}

fn params<'a>(instance: &'a str, limit: u32) -> ClaimParams<'a> {
    ClaimParams {
        instance_id: instance,
        limit,
        lease_ttl: TTL,
        control_kinds: Effect::CONTROL_KINDS,
    }
}

fn claim(store: &mut Store, instance: &str, limit: u32) -> Vec<turtle_store::EffectRow> {
    store
        .transaction(|tx| tx.claim_effects(&params(instance, limit)))
        .expect("claim")
}

/// Claims and completes everything currently runnable.
fn drain(store: &mut Store, instance: &str) {
    loop {
        let batch = claim(store, instance, 50);
        if batch.is_empty() {
            return;
        }
        for e in batch {
            store
                .transaction(|tx| tx.complete_effect(e.id, instance, &serde_json::json!({})))
                .expect("complete");
        }
    }
}

/// Drives a job to `implementation`, leaving [create_worktree, dispatch_ready_tasks] pending.
fn to_implementation(store: &mut Store, inst: &str, mut job: JobRecord) -> JobRecord {
    for cmd in [
        C::Start,
        C::SpecProduced {
            valid: true,
            criteria_count: 2,
        },
        C::ApprovalGranted {
            gate: GateKind::Scope,
        },
    ] {
        job = submit(store, &job, &cmd);
        drain(store, inst);
    }
    submit(
        store,
        &job,
        &C::PlanProduced {
            valid: true,
            within_scope: true,
        },
    )
}

fn kinds(rows: &[turtle_store::EffectRow]) -> Vec<&str> {
    rows.iter().map(|e| e.kind.as_str()).collect()
}

#[test]
fn effects_of_one_job_run_in_order_and_one_at_a_time() {
    let (mut s, _c, job, inst) = setup();
    let job = to_implementation(&mut s, &inst, job);

    let first = claim(&mut s, &inst, 10);
    assert_eq!(
        kinds(&first),
        ["create_worktree"],
        "only the first effect of the job"
    );
    assert_eq!(first[0].attempts, 1);
    assert!(
        claim(&mut s, &inst, 10).is_empty(),
        "nothing else while it is claimed"
    );

    s.transaction(|tx| tx.complete_effect(first[0].id, &inst, &serde_json::json!({"ok": true})))
        .expect("complete");
    let second = claim(&mut s, &inst, 10);
    assert_eq!(kinds(&second), ["dispatch_ready_tasks"]);
    let _ = job;
}

#[test]
fn jobs_with_holds_get_no_new_work_but_resume_restores_it() {
    let (mut s, _c, job, inst) = setup();
    let job = submit(&mut s, &job, &C::Start); // pending: start_run
    let job = submit(
        &mut s,
        &job,
        &C::Pause {
            mode: PauseMode::Soft,
        },
    );
    assert!(
        claim(&mut s, &inst, 10).is_empty(),
        "paused job must not start runs"
    );
    submit(&mut s, &job, &C::Resume);
    assert_eq!(kinds(&claim(&mut s, &inst, 10)), ["start_run"]);
}

#[test]
fn control_effects_run_for_held_blocked_and_cancelled_jobs() {
    let (mut s, _c, job, inst) = setup();
    let job = submit(&mut s, &job, &C::Start); // pending start_run
    // Hard pause adds terminate_runs, which must run although the job is held.
    let job = submit(
        &mut s,
        &job,
        &C::Pause {
            mode: PauseMode::Hard,
        },
    );
    let got = claim(&mut s, &inst, 10);
    assert_eq!(
        kinds(&got),
        ["terminate_runs"],
        "start_run stays held, terminate_runs runs"
    );
    for e in got {
        s.transaction(|tx| tx.complete_effect(e.id, &inst, &serde_json::json!({})))
            .expect("done");
    }

    // Cancelling drops the stale start_run and runs the cleanup effects.
    let job = submit(&mut s, &job, &C::Cancel);
    let got = claim(&mut s, &inst, 50);
    let mut k = kinds(&got);
    k.sort_unstable();
    assert_eq!(
        k,
        [
            "cleanup_worktree",
            "release_reservations",
            "supersede_pending_approvals",
            "terminate_runs"
        ]
    );
    let all = s.list_effects(job.job.id).expect("effects");
    let start = all
        .iter()
        .find(|e| e.kind == "start_run")
        .expect("start_run exists");
    assert_eq!(
        start.status, "cancelled",
        "stale work of a finished job is cancelled, not run"
    );
}

#[test]
fn blocked_job_drops_pending_work_but_notifies() {
    let (mut s, _c, job, inst) = setup();
    let job = submit(&mut s, &job, &C::Start);
    submit(
        &mut s,
        &job,
        &C::Halt {
            reason: BlockReason::Timeout,
        },
    );
    let k = kinds(&claim(&mut s, &inst, 10))
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    assert!(
        k.contains(&"notify_user".to_string()) && k.contains(&"release_reservations".to_string()),
        "{k:?}"
    );
    assert!(!k.contains(&"start_run".to_string()));
}

#[test]
fn only_the_lease_holder_can_complete_or_fail() {
    let (mut s, _c, job, inst) = setup();
    submit(&mut s, &job, &C::Start);
    let e = claim(&mut s, &inst, 1).remove(0);
    let other = s
        .transaction(|tx| tx.register_instance("test", 2))
        .expect("instance");

    let err = s
        .transaction(|tx| tx.complete_effect(e.id, &other, &serde_json::json!({})))
        .expect_err("not the holder");
    assert!(matches!(err, StoreError::LeaseLost { .. }), "{err}");
    let err = s
        .transaction(|tx| tx.fail_effect(e.id, &other, "boom", None))
        .expect_err("not the holder");
    assert!(matches!(err, StoreError::LeaseLost { .. }));

    s.transaction(|tx| tx.complete_effect(e.id, &inst, &serde_json::json!({})))
        .expect("holder completes");
    let err = s
        .transaction(|tx| tx.complete_effect(e.id, &inst, &serde_json::json!({})))
        .expect_err("already completed");
    assert!(
        matches!(err, StoreError::LeaseLost { .. }),
        "completion is not repeatable"
    );
}

#[test]
fn renew_lease_only_for_the_holder() {
    let (mut s, _c, job, inst) = setup();
    submit(&mut s, &job, &C::Start);
    let e = claim(&mut s, &inst, 1).remove(0);
    assert!(
        s.transaction(|tx| tx.renew_lease(e.id, &inst, TTL))
            .expect("renew")
    );
    assert!(
        !s.transaction(|tx| tx.renew_lease(e.id, "someone-else", TTL))
            .expect("renew")
    );
}

#[test]
fn retryable_failure_backs_off_then_fails_after_max_attempts() {
    let (mut s, clock, job, inst) = setup();
    submit(&mut s, &job, &C::Start);

    for attempt in 1..=5u32 {
        let e = claim(&mut s, &inst, 1);
        assert_eq!(e.len(), 1, "attempt {attempt} must be claimable");
        assert_eq!(e[0].attempts, attempt);
        let fate = s
            .transaction(|tx| {
                tx.fail_effect(e[0].id, &inst, "transient", Some(Duration::from_secs(10)))
            })
            .expect("fail");
        if attempt < 5 {
            assert_eq!(fate, EffectFate::Requeued);
            assert!(
                claim(&mut s, &inst, 1).is_empty(),
                "backoff not yet elapsed"
            );
            clock.advance(Duration::from_secs(10));
        } else {
            assert_eq!(fate, EffectFate::Failed, "attempts exhausted");
        }
    }
    assert!(claim(&mut s, &inst, 1).is_empty());
    let events = s.list_events(None).expect("events");
    assert_eq!(
        events
            .iter()
            .filter(|e| e.kind == "effect.requeued")
            .count(),
        4
    );
    assert_eq!(
        events.iter().filter(|e| e.kind == "effect.failed").count(),
        1
    );
}

#[test]
fn non_retryable_failure_fails_immediately() {
    let (mut s, _c, job, inst) = setup();
    submit(&mut s, &job, &C::Start);
    let e = claim(&mut s, &inst, 1).remove(0);
    let fate = s
        .transaction(|tx| tx.fail_effect(e.id, &inst, "fatal", None))
        .expect("fail");
    assert_eq!(fate, EffectFate::Failed);
}

#[test]
fn restart_recovery_requeues_effects_claimed_by_a_dead_instance() {
    let (mut s, _c, job, dead) = setup();
    submit(&mut s, &job, &C::Start);
    let e = claim(&mut s, &dead, 1).remove(0); // "crash": never completed

    let alive = s
        .transaction(|tx| tx.register_instance("test", 2))
        .expect("instance");
    let report = s
        .transaction(|tx| tx.recover_after_restart(&alive))
        .expect("recover");
    assert_eq!(report.instances_stopped, 1);
    assert_eq!(report.effects_requeued, 1);
    assert!(report.effects_failed.is_empty());

    let again = claim(&mut s, &alive, 1);
    assert_eq!(again.len(), 1);
    assert_eq!(again[0].id, e.id, "same effect, same idempotency key");
    assert_eq!(again[0].idempotency_key, e.idempotency_key);
    assert_eq!(again[0].attempts, 2, "the abandoned attempt counts");

    // The dead instance can no longer complete it.
    let err = s
        .transaction(|tx| tx.complete_effect(e.id, &dead, &serde_json::json!({})))
        .expect_err("lost");
    assert!(matches!(err, StoreError::LeaseLost { .. }));
    // Recovery is idempotent and leaves the live instance's work alone.
    let report = s
        .transaction(|tx| tx.recover_after_restart(&alive))
        .expect("recover again");
    assert_eq!(report.effects_requeued, 0);
    assert_eq!(
        s.list_effects(job.job.id)
            .expect("effects")
            .iter()
            .filter(|x| x.status == "claimed")
            .count(),
        1
    );
    assert!(matches!(
        s.verify_chain().expect("verify"),
        turtle_store::ChainVerdict::Intact { .. }
    ));
}

#[test]
fn recovery_fails_effects_that_already_used_every_attempt() {
    let (mut s, clock, job, dead) = setup();
    submit(&mut s, &job, &C::Start);
    for _ in 0..4 {
        let e = claim(&mut s, &dead, 1).remove(0);
        s.transaction(|tx| tx.fail_effect(e.id, &dead, "transient", Some(Duration::from_secs(1))))
            .expect("fail");
        clock.advance(Duration::from_secs(2));
    }
    let last = claim(&mut s, &dead, 1).remove(0);
    assert_eq!(last.attempts, 5);

    let alive = s
        .transaction(|tx| tx.register_instance("test", 2))
        .expect("instance");
    let report = s
        .transaction(|tx| tx.recover_after_restart(&alive))
        .expect("recover");
    assert_eq!(report.effects_requeued, 0);
    assert_eq!(report.effects_failed.len(), 1);
    assert_eq!(report.effects_failed[0].id, last.id);
    assert!(claim(&mut s, &alive, 10).is_empty());
}
