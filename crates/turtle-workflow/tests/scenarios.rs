//! End-to-end command sequences per profile (WORKFLOW.md §3–§6).

mod common;

use common::{hostile, job_id, permissive, scheduler, user};
use turtle_domain::{Actor, ApprovalValidity, Complexity, GateKind, JobState, Role, RunId};
use turtle_workflow::{
    BlockReason, DecisionContext, Effect, Hold, JobAggregate, JobCommand as C, PauseMode, Profile,
    ReviewVerdict, RunPurpose, VerificationOutcome, decide,
};

/// Applies a command and returns the new job, panicking with the guard on rejection.
fn step(job: &JobAggregate, cmd: C, actor: &Actor, cx: &DecisionContext) -> JobAggregate {
    match decide(job, &cmd, actor, cx) {
        Ok(d) => d.job,
        Err(r) => panic!("{r}"),
    }
}

fn effects(job: &JobAggregate, cmd: C, actor: &Actor, cx: &DecisionContext) -> Vec<Effect> {
    decide(job, &cmd, actor, cx).expect("accepted").effects
}

fn spec() -> C {
    C::SpecProduced {
        valid: true,
        criteria_count: 5,
    }
}

fn plan() -> C {
    C::PlanProduced {
        valid: true,
        within_scope: true,
    }
}

/// Drives a job from Backlog to Done; returns the visited states.
fn happy_path(profile: Profile, complexity: Option<Complexity>) -> Vec<JobState> {
    let cx = permissive();
    let mut job = JobAggregate::new(job_id(), profile);
    job.complexity = complexity;
    let mut states = vec![job.state];
    let mut go = |job: &mut JobAggregate, cmd: C, actor: Actor| {
        *job = step(job, cmd, &actor, &cx);
        states.push(job.state);
    };
    go(&mut job, C::Start, user());
    if job.state == JobState::Discovery {
        go(
            &mut job,
            C::DiscoveryCompleted { report_valid: true },
            scheduler(),
        );
    }
    go(&mut job, spec(), scheduler());
    go(
        &mut job,
        C::ApprovalGranted {
            gate: GateKind::Scope,
        },
        user(),
    );
    go(&mut job, plan(), scheduler());
    if job.state == JobState::PlanApproval {
        go(
            &mut job,
            C::ApprovalGranted {
                gate: GateKind::Plan,
            },
            user(),
        );
    }
    go(&mut job, C::AllTasksSucceeded, scheduler());
    go(
        &mut job,
        C::GatesFinished {
            all_required_passed: true,
        },
        scheduler(),
    );
    go(
        &mut job,
        C::ReviewCompleted {
            verdict: ReviewVerdict::Pass,
        },
        scheduler(),
    );
    go(
        &mut job,
        C::VerificationCompleted {
            outcome: VerificationOutcome::Met,
        },
        scheduler(),
    );
    go(
        &mut job,
        C::ApprovalGranted {
            gate: GateKind::Acceptance,
        },
        user(),
    );
    go(&mut job, C::Delivered, scheduler());
    assert_eq!(job.version as usize, states.len() - 1);
    states
}

#[test]
fn standard_happy_path_small_job() {
    use JobState::*;
    assert_eq!(
        happy_path(Profile::standard(), Some(Complexity::S)),
        [
            Backlog,
            Specification,
            ScopeApproval,
            Planning,
            Implementation,
            BuildTest,
            Review,
            Verification,
            AcceptanceApproval,
            Delivery,
            Done
        ]
    );
}

#[test]
fn strict_adds_discovery_and_plan_approval() {
    let states = happy_path(Profile::strict(), Some(Complexity::S));
    assert!(states.contains(&JobState::Discovery));
    assert!(states.contains(&JobState::PlanApproval));
    assert_eq!(states.last(), Some(&JobState::Done));
}

#[test]
fn standard_runs_discovery_for_medium_jobs() {
    assert!(happy_path(Profile::standard(), Some(Complexity::M)).contains(&JobState::Discovery));
}

#[test]
fn entering_implementation_creates_worktree_and_dispatches_tasks() {
    let job = common::job_in(JobState::Planning, Profile::standard());
    assert_eq!(
        effects(&job, plan(), &scheduler(), &permissive()),
        [Effect::CreateWorktree, Effect::DispatchReadyTasks]
    );
}

#[test]
fn fast_profile_auto_approves_small_scope_only_when_rule_holds() {
    let job = common::job_in(JobState::Specification, Profile::fast());
    let mut cx = permissive();
    cx.scope_auto_approvable = true;
    let e = effects(&job, spec(), &scheduler(), &cx);
    assert!(e.contains(&Effect::RequestApproval {
        gate: GateKind::Scope,
        auto: true
    }));

    let in_scope = common::job_in(JobState::ScopeApproval, Profile::fast());
    assert!(
        decide(
            &in_scope,
            &C::ApprovalGranted {
                gate: GateKind::Scope
            },
            &Actor::System,
            &cx
        )
        .is_ok()
    );
    cx.scope_auto_approvable = false;
    let r = decide(
        &in_scope,
        &C::ApprovalGranted {
            gate: GateKind::Scope,
        },
        &Actor::System,
        &cx,
    )
    .expect_err("rule does not hold");
    assert_eq!(r.guard, "auto_scope_allowed");
}

#[test]
fn standard_profile_never_auto_approves_scope() {
    let job = common::job_in(JobState::ScopeApproval, Profile::standard());
    let mut cx = permissive();
    cx.scope_auto_approvable = true;
    let r = decide(
        &job,
        &C::ApprovalGranted {
            gate: GateKind::Scope,
        },
        &Actor::System,
        &cx,
    )
    .expect_err("standard requires a human");
    assert_eq!(r.guard, "auto_scope_allowed");
}

#[test]
fn acceptance_can_only_be_granted_by_a_user_with_valid_binding() {
    let job = common::job_in(JobState::AcceptanceApproval, Profile::fast());
    let grant = C::ApprovalGranted {
        gate: GateKind::Acceptance,
    };
    assert_eq!(
        decide(&job, &grant, &Actor::System, &permissive())
            .expect_err("system")
            .guard,
        "user_decides_approval"
    );
    assert_eq!(
        decide(
            &job,
            &grant,
            &Actor::AgentRun { id: RunId::new() },
            &permissive()
        )
        .expect_err("agent")
        .guard,
        "agents_cannot_issue_commands"
    );
    let mut stale = permissive();
    stale
        .approvals
        .insert(GateKind::Acceptance, ApprovalValidity::Invalid);
    assert_eq!(
        decide(&job, &grant, &user(), &stale)
            .expect_err("stale")
            .guard,
        "approval_valid"
    );
    let mut unacked = permissive();
    unacked.flags_acknowledged = false;
    assert_eq!(
        decide(&job, &grant, &user(), &unacked)
            .expect_err("flags")
            .guard,
        "flags_acknowledged"
    );
}

#[test]
fn users_cannot_fake_machine_results() {
    let job = common::job_in(JobState::BuildTest, Profile::standard());
    let r = decide(
        &job,
        &C::GatesFinished {
            all_required_passed: true,
        },
        &user(),
        &permissive(),
    )
    .expect_err("user cannot report gates");
    assert_eq!(r.guard, "machine_result_only");
}

#[test]
fn fix_loop_blocks_after_limit() {
    let cx = permissive();
    let mut job = common::job_in(JobState::BuildTest, Profile::standard());
    for i in 1..=3 {
        job = step(
            &job,
            C::GatesFinished {
                all_required_passed: false,
            },
            &scheduler(),
            &cx,
        );
        assert_eq!(job.state, JobState::Implementation);
        assert_eq!(job.counters.fix_iterations, i);
        job = step(&job, C::AllTasksSucceeded, &scheduler(), &cx);
    }
    job = step(
        &job,
        C::GatesFinished {
            all_required_passed: false,
        },
        &scheduler(),
        &cx,
    );
    assert_eq!(job.state, JobState::Blocked);
    let info = job.blocked.clone().expect("block info");
    assert_eq!(info.reason, BlockReason::FixLimitReached);
    assert_eq!(info.from, JobState::BuildTest);

    // A human resolves the block: fresh fix budget, back to implementation.
    let resolved = step(
        &job,
        C::ResolveBlock {
            target: JobState::Implementation,
        },
        &user(),
        &cx,
    );
    assert_eq!(resolved.state, JobState::Implementation);
    assert_eq!(resolved.counters.fix_iterations, 0);

    let r = decide(
        &job,
        &C::ResolveBlock {
            target: JobState::Delivery,
        },
        &user(),
        &cx,
    )
    .expect_err("not an allowed target");
    assert_eq!(r.guard, "target_allowed");
}

#[test]
fn review_fix_requests_fixer_run() {
    let job = common::job_in(JobState::Review, Profile::standard());
    let e = effects(
        &job,
        C::ReviewCompleted {
            verdict: ReviewVerdict::Fixable,
        },
        &scheduler(),
        &permissive(),
    );
    assert_eq!(
        e,
        [Effect::StartRun {
            role: Role::Fixer,
            purpose: RunPurpose::Fix
        }]
    );
}

#[test]
fn user_changes_requested_does_not_consume_loop_budget() {
    let cx = permissive();
    let mut job = common::job_in(JobState::AcceptanceApproval, Profile::fast());
    for _ in 0..20 {
        job = step(
            &job,
            C::ApprovalChangesRequested {
                gate: GateKind::Acceptance,
            },
            &user(),
            &cx,
        );
        assert_eq!(job.state, JobState::Implementation);
        job.state = JobState::AcceptanceApproval;
    }
    assert_eq!(job.counters.back_transitions, 0);
}

#[test]
fn spec_edit_during_scope_approval_re_requests_approval() {
    let job = common::job_in(JobState::ScopeApproval, Profile::standard());
    let d = decide(&job, &spec(), &user(), &permissive()).expect("accepted");
    assert_eq!(d.job.state, JobState::ScopeApproval);
    assert!(d.effects.contains(&Effect::RequestApproval {
        gate: GateKind::Scope,
        auto: false
    }));
}

#[test]
fn waits_for_dependencies_before_implementation() {
    let mut cx = permissive();
    cx.dependencies_delivered = false;
    let job = common::job_in(JobState::Planning, Profile::standard());
    let waiting = step(&job, plan(), &scheduler(), &cx);
    assert_eq!(waiting.state, JobState::Planning);
    assert_eq!(waiting.holds, [Hold::WaitingForDependencies]);
    assert!(!waiting.is_runnable());

    let started = step(
        &waiting,
        C::DependenciesDelivered,
        &scheduler(),
        &permissive(),
    );
    assert_eq!(started.state, JobState::Implementation);
    assert!(started.holds.is_empty());
}

#[test]
fn hard_pause_terminates_runs_and_resume_clears_hold() {
    let cx = permissive();
    let job = common::job_in(JobState::Implementation, Profile::standard());
    let d = decide(
        &job,
        &C::Pause {
            mode: PauseMode::Hard,
        },
        &user(),
        &cx,
    )
    .expect("pause");
    assert!(d.effects.contains(&Effect::TerminateRuns));
    assert_eq!(
        d.job.state,
        JobState::Implementation,
        "pause keeps the stage"
    );
    assert_eq!(
        decide(
            &d.job,
            &C::Pause {
                mode: PauseMode::Soft
            },
            &user(),
            &cx
        )
        .expect_err("twice")
        .guard,
        "not_already_paused"
    );
    let resumed = step(&d.job, C::Resume, &user(), &cx);
    assert!(resumed.is_runnable());
}

#[test]
fn preflight_deny_blocks_and_missing_preflight_rejects() {
    let job = common::job_in(JobState::Specification, Profile::standard());
    assert_eq!(
        decide(&job, &spec(), &scheduler(), &hostile())
            .expect_err("no preflight")
            .guard,
        "preflight_computed"
    );
    let mut cx = permissive();
    cx.preflight = Some(turtle_workflow::PreflightDecision::Deny);
    let blocked = step(&job, spec(), &scheduler(), &cx);
    assert_eq!(blocked.state, JobState::Blocked);
}

#[test]
fn cancel_cleans_up_and_job_becomes_immutable() {
    let job = common::job_in(JobState::Review, Profile::standard());
    let d = decide(&job, &C::Cancel, &user(), &permissive()).expect("cancel");
    assert_eq!(d.job.state, JobState::Cancelled);
    assert!(d.effects.contains(&Effect::TerminateRuns));
    assert!(d.effects.contains(&Effect::ReleaseReservations));
    let r = decide(&d.job, &C::Resume, &user(), &permissive()).expect_err("terminal");
    assert_eq!(r.guard, "job_not_terminal");
}

#[test]
fn base_refresh_invalidates_acceptance_and_reruns_gates() {
    let job = common::job_in(JobState::Delivery, Profile::standard());
    let d = decide(&job, &C::BaseRefreshed, &scheduler(), &permissive()).expect("refresh");
    assert_eq!(d.job.state, JobState::BuildTest);
    assert_eq!(d.effects, [Effect::RunGates]);
}

#[test]
fn release_required_waits_for_release() {
    let profile = Profile {
        release_required: true,
        ..Profile::standard()
    };
    let job = common::job_in(JobState::Delivery, profile);
    let delivered = step(&job, C::Delivered, &scheduler(), &permissive());
    assert_eq!(delivered.state, JobState::AwaitingRelease);
    let done = step(&delivered, C::ReleaseCompleted, &scheduler(), &permissive());
    assert_eq!(done.state, JobState::Done);
}

#[test]
fn changes_requested_while_waiting_for_dependencies_does_not_strand_the_hold() {
    let mut cx = permissive();
    cx.plan_thresholds_exceeded = true;
    cx.dependencies_delivered = false;
    let job = common::job_in(JobState::Planning, Profile::strict());

    let awaiting = step(&job, plan(), &scheduler(), &cx);
    assert_eq!(awaiting.state, JobState::PlanApproval);
    let waiting = step(
        &awaiting,
        C::ApprovalGranted {
            gate: GateKind::Plan,
        },
        &user(),
        &cx,
    );
    assert_eq!(waiting.holds, [Hold::WaitingForDependencies]);

    let revised = step(
        &waiting,
        C::ApprovalChangesRequested {
            gate: GateKind::Plan,
        },
        &user(),
        &cx,
    );
    assert_eq!(revised.state, JobState::Planning);
    assert!(
        revised.holds.is_empty(),
        "the wait ends when the plan is sent back"
    );
    assert!(!revised.ready_for_implementation);

    // The next plan and approval go through without a duplicate-hold rejection.
    let again = step(&revised, plan(), &scheduler(), &cx);
    let waiting_again = step(
        &again,
        C::ApprovalGranted {
            gate: GateKind::Plan,
        },
        &user(),
        &cx,
    );
    assert_eq!(waiting_again.holds, [Hold::WaitingForDependencies]);
    let started = step(
        &waiting_again,
        C::DependenciesDelivered,
        &scheduler(),
        &permissive(),
    );
    assert_eq!(started.state, JobState::Implementation);
}

#[test]
fn effect_kind_matches_serialized_tag_and_control_kinds_are_known() {
    let samples = [
        Effect::StartRun {
            role: Role::Planner,
            purpose: RunPurpose::Initial,
        },
        Effect::RequestApproval {
            gate: GateKind::Scope,
            auto: false,
        },
        Effect::CreateWorktree,
        Effect::DispatchReadyTasks,
        Effect::RunGates,
        Effect::RunVerification,
        Effect::Deliver,
        Effect::CleanupWorktree,
        Effect::TerminateRuns,
        Effect::ReleaseReservations,
        Effect::SupersedePendingApprovals,
        Effect::NotifyUser,
    ];
    for e in &samples {
        let tag = serde_json::to_value(e).expect("serialize")["effect"]
            .as_str()
            .expect("tag")
            .to_owned();
        assert_eq!(e.kind(), tag);
    }
    for kind in Effect::CONTROL_KINDS {
        assert!(
            samples.iter().any(|e| e.kind() == *kind),
            "unknown control kind {kind}"
        );
    }
    assert!(Effect::TerminateRuns.is_control() && !Effect::Deliver.is_control());
}
