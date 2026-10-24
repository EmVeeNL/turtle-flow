//! Crash-injection tests (TST-12, `docs/TESTING-STRATEGY.md` §6).
//!
//! A scripted job (two human approvals, a dozen effects) runs on a file-backed database. The
//! scheduler is "killed" at the Nth step boundary, a brand-new instance starts on the same
//! database and recovers, and the run continues. For every N the job must end in the same state
//! with exactly the same event history as an uninterrupted run: nothing lost, nothing applied
//! twice, every effect finished.

mod common;

use std::collections::BTreeMap;
use std::sync::Arc;

use common::*;
use turtle_domain::{Actor, GateKind, JobId, JobState};
use turtle_scheduler::{CrashPlan, DispatchError, EngineError};
use turtle_store::{ChainVerdict, Store, SystemClock};
use turtle_workflow::{JobCommand as C, Profile};

#[derive(Debug)]
struct Outcome {
    /// `job.*` events as `kind:detail`, in order (effect and daemon events excluded).
    history: Vec<String>,
    effect_keys: Vec<String>,
    executions: BTreeMap<String, u32>,
    restarts: u32,
    requeued_by_recovery: u32,
    /// Step boundaries the first instance passed.
    boundaries: usize,
    first_crash_fired: bool,
}

fn is_crash(e: &DispatchError) -> bool {
    matches!(
        e,
        DispatchError::Crashed(_) | DispatchError::Engine(EngineError::Crashed(_))
    )
}

async fn boot(
    path: &std::path::Path,
    exec: &Arc<SimExecutor>,
    plan: CrashPlan,
) -> (TestDispatcher, turtle_store::RecoveryReport) {
    let store = Store::open(path, Arc::new(SystemClock)).expect("open store");
    start_dispatcher(&turtle_scheduler::SharedStore::new(store), exec, plan).await
}

/// One scheduler step: perform any due human action, then one dispatcher tick.
/// `Ok(true)` once the job is done and every effect has finished.
async fn step(d: &TestDispatcher, job: JobId) -> Result<bool, DispatchError> {
    // Human actions, retried on every step just like a person would retry after a failure.
    let command = match state_of(d, job).await {
        JobState::Backlog => Some(C::Start),
        JobState::ScopeApproval => Some(C::ApprovalGranted {
            gate: GateKind::Scope,
        }),
        JobState::AcceptanceApproval => Some(C::ApprovalGranted {
            gate: GateKind::Acceptance,
        }),
        _ => None,
    };
    if let Some(cmd) = command {
        match d.engine().submit(job, cmd, Actor::local_user()).await {
            Ok(_) | Err(EngineError::Rejected(_)) => {}
            Err(e) => return Err(e.into()),
        }
    }
    d.tick().await?;
    let unfinished = d
        .engine()
        .store()
        .with(|s| s.transaction(|tx| tx.unfinished_effect_count()))
        .await
        .expect("count");
    Ok(state_of(d, job).await == JobState::Done && unfinished == 0)
}

/// Runs the scripted job, crashing at the given boundaries (one per instance, in order).
async fn scenario(crashes: &[usize]) -> Outcome {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("turtle.db");
    let store = turtle_scheduler::SharedStore::new(
        Store::open(&path, Arc::new(SystemClock)).expect("open"),
    );
    let job = create_job(&store, Profile::standard()).await;
    drop(store);

    let world = Arc::new(World::default());
    let exec = SimExecutor::new(Arc::clone(&world));
    let mut plans = crashes.iter().map(|&n| CrashPlan::at(n));
    let first = plans.next().unwrap_or_else(CrashPlan::never);
    let (mut d, _) = boot(&path, &exec, first.clone()).await;

    let (mut restarts, mut requeued) = (0u32, 0u32);
    for _ in 0..400 {
        match step(&d, job).await {
            Ok(true) => break,
            Ok(false) => {}
            Err(e) if is_crash(&e) => {
                restarts += 1;
                drop(d);
                let next = plans.next().unwrap_or_else(CrashPlan::never);
                let (nd, report) = boot(&path, &exec, next).await;
                requeued += report.effects_requeued;
                d = nd;
            }
            Err(e) => panic!("unexpected error: {e}"),
        }
    }

    let (events, effects, verdict) = d
        .engine()
        .store()
        .with(move |s| {
            (
                s.list_events(Some(job)).expect("events"),
                s.list_effects(job).expect("effects"),
                s.verify_chain().expect("verify"),
            )
        })
        .await;
    assert!(
        matches!(verdict, ChainVerdict::Intact { .. }),
        "chain broken after crashes {crashes:?}"
    );
    assert_eq!(
        state_of(&d, job).await,
        JobState::Done,
        "crashes {crashes:?}"
    );
    assert!(
        effects.iter().all(|e| e.status == "completed"),
        "crashes {crashes:?}: {:?}",
        effects
            .iter()
            .map(|e| (&e.kind, &e.status))
            .collect::<Vec<_>>()
    );
    assert!(
        events.iter().all(|e| e.kind != "effect.failed"),
        "crashes {crashes:?}"
    );

    let history = events
        .iter()
        .filter(|e| e.kind.starts_with("job."))
        .map(|e| {
            let detail = e.payload["command"]
                .as_str()
                .or_else(|| e.payload["to"].as_str())
                .unwrap_or("");
            format!("{}:{detail}", e.kind)
        })
        .collect();
    // Keys look like `job:<id>:v3:e1`; the job id differs per run, the rest must not.
    let strip = |key: &str| key.splitn(3, ':').nth(2).unwrap_or(key).to_owned();
    let mut effect_keys: Vec<String> = effects.iter().map(|e| strip(&e.idempotency_key)).collect();
    effect_keys.sort();
    let executions = world
        .executions
        .lock()
        .expect("lock")
        .iter()
        .map(|(k, v)| (strip(k), *v))
        .collect();
    Outcome {
        history,
        effect_keys,
        executions,
        restarts,
        requeued_by_recovery: requeued,
        boundaries: first.boundaries_seen(),
        first_crash_fired: first.fired(),
    }
}

#[tokio::test]
async fn an_uninterrupted_run_is_the_baseline() {
    let base = scenario(&[]).await;
    assert_eq!(base.restarts, 0);
    assert!(
        base.boundaries >= 30,
        "too few step boundaries to be a meaningful harness: {}",
        base.boundaries
    );
    assert!(base.executions.values().all(|&n| n == 1));
    assert!(base.history.contains(&"job.stage_entered:done".to_owned()));
    assert_eq!(
        base.history.first().map(String::as_str),
        Some("job.created:")
    );
}

#[tokio::test]
async fn crashing_at_every_step_boundary_converges_to_the_same_result() {
    let base = scenario(&[]).await;
    let mut reran = 0;
    let mut recovered = 0;
    for n in 0..base.boundaries {
        let run = scenario(&[n]).await;
        assert!(run.first_crash_fired, "boundary {n} was not reached");
        assert_eq!(run.restarts, 1, "boundary {n}");
        assert_eq!(
            run.history, base.history,
            "history differs after a crash at boundary {n}"
        );
        assert_eq!(
            run.effect_keys, base.effect_keys,
            "effect set differs after a crash at boundary {n}"
        );
        assert!(
            run.executions.values().all(|&c| c >= 1),
            "an effect never ran after a crash at boundary {n}"
        );
        assert_eq!(
            run.executions.keys().collect::<Vec<_>>(),
            base.executions.keys().collect::<Vec<_>>()
        );
        reran += usize::from(run.executions.values().any(|&c| c > 1));
        recovered += usize::from(run.requeued_by_recovery > 0);
    }
    // The harness must really exercise at-least-once delivery and recovery, not just pass.
    assert!(
        reran >= 3,
        "only {reran} crash points re-ran an effect; harness is too weak"
    );
    assert!(
        recovered >= 3,
        "only {recovered} crash points needed recovery to requeue an effect"
    );
}

#[tokio::test]
async fn two_crashes_in_a_row_still_converge() {
    let base = scenario(&[]).await;
    for first in (0..base.boundaries).step_by(2) {
        for second in [0, 3, 8] {
            let run = scenario(&[first, second]).await;
            assert_eq!(
                run.history, base.history,
                "crashes at {first} then {second}"
            );
            assert_eq!(
                run.effect_keys, base.effect_keys,
                "crashes at {first} then {second}"
            );
            assert!(run.restarts >= 1);
        }
    }
}
