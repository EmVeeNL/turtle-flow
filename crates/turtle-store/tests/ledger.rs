//! Budgets, reservations and the ledger: atomic reservation across scopes (B-1), idempotent
//! settlement, overrun honesty, monthly rollover, abandoned runs and tamper detection.

use std::sync::Arc;
use std::time::Duration;

use turtle_domain::{
    Actor, JobId, JobType, Priority, ProjectId, Role, RunId, TokenUsage, UsageValue,
};
use turtle_store::{
    BudgetScope, JobUsage, LedgerVerdict, ManualClock, NewJob, NewProject, NewRun,
    ReservationOutcome, RunEnd, Store,
};
use turtle_workflow::Profile;

struct World {
    store: Store,
    clock: Arc<ManualClock>,
    project: ProjectId,
    job: JobId,
}

fn world() -> World {
    let clock = Arc::new(ManualClock::new());
    let mut store = Store::open_in_memory(clock.clone()).expect("store");
    let (project, job) = store
        .transaction(|tx| {
            let project = tx.insert_project(&NewProject {
                slug: "payments-api".into(),
                key_prefix: "API".into(),
                name: "Payments".into(),
                description: String::new(),
                repo_path: "/tmp/p".into(),
                default_branch: "main".into(),
                default_profile: "standard".into(),
            })?;
            let job = tx
                .create_job(
                    &NewJob {
                        project_id: project,
                        title: "Add limiter".into(),
                        description: String::new(),
                        job_type: JobType::Feature,
                        priority: Priority::Normal,
                        profile: Profile::standard(),
                    },
                    &Actor::local_user(),
                )?
                .job
                .id;
            Ok((project, job))
        })
        .expect("seed");
    World {
        store,
        clock,
        project,
        job,
    }
}

fn run(w: &mut World) -> RunId {
    let id = RunId::new();
    let job = w.job;
    w.store
        .transaction(|tx| {
            tx.begin_run(&NewRun {
                id,
                job_id: job,
                role: Role::Implementer,
                stage: "implementation".into(),
                provider: "fake".into(),
                model: "fake-1".into(),
                adapter_version: "0".into(),
            })
        })
        .expect("begin run");
    id
}

fn set(w: &mut World, scope: BudgetScope, limit: u64, hard: bool) {
    w.store
        .transaction(|tx| tx.set_budget(scope, limit, hard, None, &Actor::local_user()))
        .expect("set budget");
}

fn reserve(w: &mut World, run: RunId, tokens: u64) -> ReservationOutcome {
    let job = w.job;
    w.store
        .transaction(|tx| tx.reserve(run, job, tokens))
        .expect("reserve")
}

fn budgets(w: &World) -> Vec<(String, u64, u64, u64)> {
    w.store
        .list_budgets()
        .expect("list")
        .into_iter()
        .map(|b| {
            let scope = match b.scope {
                BudgetScope::Global => "global",
                BudgetScope::Project(_) => "project",
                BudgetScope::Job(_) => "job",
            };
            (
                scope.to_owned(),
                b.limit_tokens,
                b.reserved_tokens,
                b.settled_tokens,
            )
        })
        .collect()
}

fn consistent(w: &World) {
    assert!(
        matches!(
            w.store.verify_ledger().expect("verify"),
            LedgerVerdict::Consistent { .. }
        ),
        "cached columns must equal the ledger"
    );
}

#[test]
fn a_job_with_no_budget_is_unmetered() {
    let mut w = world();
    let r = run(&mut w);
    assert_eq!(reserve(&mut w, r, 10_000), ReservationOutcome::Unmetered);
    assert!(
        !w.store.transaction(|tx| tx.settle(r, 5)).expect("settle"),
        "nothing to settle"
    );
}

#[test]
fn reservation_is_all_or_nothing_across_scopes() {
    let mut w = world();
    let (project, job) = (w.project, w.job);
    set(&mut w, BudgetScope::Project(project), 1_000, true);
    set(&mut w, BudgetScope::Job(job), 600, true);

    let first = run(&mut w);
    assert!(matches!(
        reserve(&mut w, first, 500),
        ReservationOutcome::Reserved { .. }
    ));
    assert_eq!(
        budgets(&w),
        vec![
            ("job".into(), 600, 500, 0),
            ("project".into(), 1_000, 500, 0)
        ]
    );

    // 200 fits the project (500 left) but not the job (100 left): nothing may be reserved.
    let second = run(&mut w);
    match reserve(&mut w, second, 200) {
        ReservationOutcome::Denied { budget, requested } => {
            assert_eq!(budget.scope, BudgetScope::Job(job));
            assert_eq!((requested, budget.headroom()), (200, 100));
        }
        other => panic!("expected a denial, got {other:?}"),
    }
    assert_eq!(
        budgets(&w),
        vec![
            ("job".into(), 600, 500, 0),
            ("project".into(), 1_000, 500, 0)
        ],
        "the project budget must not keep a reservation the job refused"
    );
    consistent(&w);
}

#[test]
fn settling_moves_reserved_to_settled_and_is_idempotent() {
    let mut w = world();
    let job = w.job;
    set(&mut w, BudgetScope::Job(job), 1_000, true);
    let r = run(&mut w);
    reserve(&mut w, r, 400);

    assert!(w.store.transaction(|tx| tx.settle(r, 250)).expect("settle"));
    assert_eq!(budgets(&w), vec![("job".into(), 1_000, 0, 250)]);
    assert!(
        !w.store.transaction(|tx| tx.settle(r, 250)).expect("again"),
        "a second settlement must not double count"
    );
    assert!(
        !w.store
            .transaction(|tx| tx.release(r, "late"))
            .expect("release")
    );
    assert_eq!(budgets(&w), vec![("job".into(), 1_000, 0, 250)]);
    consistent(&w);
}

#[test]
fn release_gives_the_headroom_back() {
    let mut w = world();
    let job = w.job;
    set(&mut w, BudgetScope::Job(job), 500, true);
    let r = run(&mut w);
    reserve(&mut w, r, 500);
    let blocked = run(&mut w);
    assert!(matches!(
        reserve(&mut w, blocked, 1),
        ReservationOutcome::Denied { .. }
    ));

    assert!(
        w.store
            .transaction(|tx| tx.release(r, "cancelled"))
            .expect("release")
    );
    assert!(matches!(
        reserve(&mut w, blocked, 500),
        ReservationOutcome::Reserved { .. }
    ));
    consistent(&w);
}

#[test]
fn an_overrun_is_booked_truthfully_and_flagged() {
    let mut w = world();
    let job = w.job;
    set(&mut w, BudgetScope::Job(job), 300, true);
    let r = run(&mut w);
    reserve(&mut w, r, 300);
    w.store.transaction(|tx| tx.settle(r, 450)).expect("settle");

    assert_eq!(
        budgets(&w),
        vec![("job".into(), 300, 0, 450)],
        "the real spend is recorded"
    );
    let events = w.store.list_events(Some(job)).expect("events");
    assert!(events.iter().any(|e| e.kind == "ledger.overrun"));
    let next = run(&mut w);
    assert!(
        matches!(reserve(&mut w, next, 1), ReservationOutcome::Denied { .. }),
        "no further runs under an exhausted hard budget"
    );
    consistent(&w);
}

#[test]
fn a_soft_budget_warns_but_does_not_stop_the_run() {
    let mut w = world();
    let (project, job) = (w.project, w.job);
    set(&mut w, BudgetScope::Project(project), 100, false);
    set(&mut w, BudgetScope::Job(job), 10_000, true);
    let r = run(&mut w);
    match reserve(&mut w, r, 500) {
        ReservationOutcome::Reserved { soft_exceeded, .. } => assert_eq!(soft_exceeded.len(), 1),
        other => panic!("expected a reservation, got {other:?}"),
    }
    consistent(&w);
}

#[test]
fn monthly_budgets_roll_over_into_a_fresh_period() {
    let mut w = world();
    let project = w.project;
    set(&mut w, BudgetScope::Project(project), 1_000, true);
    let r = run(&mut w);
    reserve(&mut w, r, 400);
    w.store.transaction(|tx| tx.settle(r, 400)).expect("settle");
    assert_eq!(budgets(&w), vec![("project".into(), 1_000, 0, 400)]);

    w.clock.advance(Duration::from_secs(40 * 24 * 3600));
    let next = run(&mut w);
    assert!(matches!(
        reserve(&mut w, next, 900),
        ReservationOutcome::Reserved { .. }
    ));
    let all = budgets(&w);
    assert_eq!(all.len(), 2, "a new period row was created: {all:?}");
    assert!(all.contains(&("project".into(), 1_000, 900, 0)), "{all:?}");
    assert!(
        all.contains(&("project".into(), 1_000, 0, 400)),
        "last month is kept: {all:?}"
    );
    consistent(&w);
}

#[test]
fn runs_left_open_by_a_dead_daemon_are_abandoned_and_released() {
    let mut w = world();
    let job = w.job;
    set(&mut w, BudgetScope::Job(job), 1_000, true);
    let r = run(&mut w);
    reserve(&mut w, r, 700);
    assert_eq!(budgets(&w), vec![("job".into(), 1_000, 700, 0)]);

    assert_eq!(
        w.store
            .transaction(|tx| tx.abandon_open_runs())
            .expect("abandon"),
        1
    );
    assert_eq!(budgets(&w), vec![("job".into(), 1_000, 0, 0)]);
    assert_eq!(
        w.store
            .transaction(|tx| tx.abandon_open_runs())
            .expect("again"),
        0
    );
    consistent(&w);
}

#[test]
fn usage_keeps_its_provenance() {
    let mut w = world();
    let usage = TokenUsage {
        input: 100,
        cached_input_read: Some(50),
        cached_input_write: None,
        output: 25,
        reasoning: None,
    };
    for (value, end) in [
        (UsageValue::Reported { usage }, RunEnd::Succeeded),
        (UsageValue::Estimated { usage }, RunEnd::Succeeded),
        (UsageValue::Unavailable, RunEnd::Failed { retryable: true }),
    ] {
        let r = run(&mut w);
        w.store
            .transaction(|tx| tx.finish_run(r, end, None, value))
            .expect("finish");
    }
    let job = w.job;
    assert_eq!(
        w.store.job_usage(job).expect("usage"),
        JobUsage {
            runs: 3,
            tokens_reported: 175,
            tokens_estimated: 175,
            runs_unavailable: 1
        },
        "an unavailable run contributes no numbers"
    );
}

#[test]
fn tampering_with_the_cached_totals_is_detected() {
    let mut w = world();
    let job = w.job;
    set(&mut w, BudgetScope::Job(job), 1_000, true);
    let r = run(&mut w);
    reserve(&mut w, r, 100);
    w.store.transaction(|tx| tx.settle(r, 100)).expect("settle");
    consistent(&w);

    w.store
        .raw_connection()
        .execute("UPDATE budgets SET settled_tokens = 1", [])
        .expect("tamper");
    assert!(matches!(
        w.store.verify_ledger().expect("verify"),
        LedgerVerdict::Mismatch { .. }
    ));
}

#[test]
fn the_budget_and_the_job_usage_agree_when_a_run_reports_reasoning() {
    let mut w = world();
    let job = w.job;
    set(&mut w, BudgetScope::Job(job), 1_000_000, true);
    let usage = TokenUsage {
        input: 4,
        cached_input_read: Some(100),
        cached_input_write: Some(50),
        output: 700,
        reasoning: Some(240),
    };
    let r = run(&mut w);
    reserve(&mut w, r, 20_000);
    w.store
        .transaction(|tx| {
            tx.finish_run(r, RunEnd::Succeeded, None, UsageValue::Reported { usage })?;
            tx.settle(r, usage.total())
        })
        .expect("finish");
    let reported = w.store.job_usage(job).expect("usage").tokens_reported;
    assert_eq!(reported, 854, "reasoning is inside the 700 output tokens");
    assert_eq!(budgets(&w), vec![("job".into(), 1_000_000, 0, reported)]);
}
