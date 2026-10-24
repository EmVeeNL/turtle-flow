//! Property tests over random command sequences (TST-03). Invariants from
//! `docs/DOMAIN-MODEL.md` §3.2 and `docs/TESTING-STRATEGY.md` §4.

mod common;

use common::sample_commands;
use proptest::prelude::*;
use turtle_domain::{Actor, ApprovalValidity, Complexity, GateKind, JobState, RunId};
use turtle_workflow::{DecisionContext, Effect, PreflightDecision, Profile, ProfileBase, decide};

#[derive(Clone, Debug)]
struct Step {
    command: usize,
    actor: u8,
    scope_valid: bool,
    plan_valid: bool,
    acceptance_valid: bool,
    preflight: u8,
    deps: bool,
    flags: bool,
    auto_scope: bool,
    thresholds: bool,
}

fn step_strategy(n_commands: usize) -> impl Strategy<Value = Step> {
    (
        0..n_commands,
        0u8..4,
        any::<bool>(),
        any::<bool>(),
        any::<bool>(),
        0u8..4,
        any::<bool>(),
        any::<bool>(),
        any::<bool>(),
        any::<bool>(),
    )
        .prop_map(
            |(
                command,
                actor,
                scope_valid,
                plan_valid,
                acceptance_valid,
                preflight,
                deps,
                flags,
                auto_scope,
                thresholds,
            )| Step {
                command,
                actor,
                scope_valid,
                plan_valid,
                acceptance_valid,
                preflight,
                deps,
                flags,
                auto_scope,
                thresholds,
            },
        )
}

fn validity(valid: bool) -> ApprovalValidity {
    if valid {
        ApprovalValidity::Valid
    } else {
        ApprovalValidity::Invalid
    }
}

fn context(s: &Step) -> DecisionContext {
    let mut cx = DecisionContext {
        preflight: match s.preflight {
            0 => None,
            1 => Some(PreflightDecision::Deny),
            2 => Some(PreflightDecision::RequireApproval),
            _ => Some(PreflightDecision::Allow),
        },
        plan_thresholds_exceeded: s.thresholds,
        dependencies_delivered: s.deps,
        flags_acknowledged: s.flags,
        scope_auto_approvable: s.auto_scope,
        ..DecisionContext::default()
    };
    cx.approvals
        .insert(GateKind::Scope, validity(s.scope_valid));
    cx.approvals.insert(GateKind::Plan, validity(s.plan_valid));
    cx.approvals
        .insert(GateKind::Acceptance, validity(s.acceptance_valid));
    cx
}

fn actor(n: u8) -> Actor {
    match n {
        0 => Actor::local_user(),
        1 => Actor::Scheduler,
        2 => Actor::System,
        _ => Actor::AgentRun { id: RunId::new() },
    }
}

fn non_terminal_states() -> Vec<JobState> {
    JobState::ALL
        .into_iter()
        .filter(|s| !s.is_terminal())
        .collect()
}

/// Command indices with destructive commands (cancel, halt, mark failed, conflict) weighted
/// down, so sequences progress through the graph instead of ending immediately.
fn weighted_command_indices() -> Vec<usize> {
    use turtle_workflow::JobCommand as C;
    let mut out = Vec::new();
    for (i, cmd) in sample_commands().iter().enumerate() {
        let destructive = matches!(
            cmd,
            C::Cancel
                | C::Halt { .. }
                | C::MarkFailed
                | C::ConflictDetected
                | C::ApprovalRejected { .. }
        );
        let copies = if destructive { 1 } else { 6 };
        out.extend(std::iter::repeat_n(i, copies));
    }
    out
}

fn profile_strategy() -> impl Strategy<Value = Profile> {
    prop_oneof![
        Just(ProfileBase::Fast),
        Just(ProfileBase::Standard),
        Just(ProfileBase::Strict)
    ]
    .prop_map(Profile::builtin)
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: std::env::var("PROPTEST_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(512),
        ..ProptestConfig::default()
    })]

    #[test]
    fn invariants_hold_for_random_command_sequences(
        profile in profile_strategy(),
        complexity in prop::option::of(prop_oneof![Just(Complexity::S), Just(Complexity::M), Just(Complexity::XL)]),
        start in prop::sample::select(non_terminal_states()),
        steps in prop::collection::vec(step_strategy(weighted_command_indices().len()), 1..60),
    ) {
        let commands = sample_commands();
        let weighted = weighted_command_indices();
        // Starting anywhere in the graph (not only at Backlog) is what lets random sequences
        // reach review, acceptance and delivery; walks from Backlog mostly end in cancel/halt.
        let mut job = common::job_in(start, profile);
        job.complexity = complexity;

        for s in &steps {
            let cmd = &commands[weighted[s.command]];
            let cx = context(s);
            let who = actor(s.actor);
            let before = job.clone();
            let result = decide(&job, cmd, &who, &cx);

            // Agents never cause transitions (invariant I-6); terminal states absorb (J-6).
            if matches!(who, Actor::AgentRun { .. }) || before.state.is_terminal() {
                prop_assert!(result.is_err());
            }

            let Ok(d) = result else { continue };
            let after = &d.job;

            prop_assert_eq!(after.version, before.version + 1);

            // J-4: no planning without a valid scope approval.
            if after.state == JobState::Planning && before.state == JobState::ScopeApproval {
                prop_assert_eq!(cx.approval(GateKind::Scope), ApprovalValidity::Valid);
            }
            // J-5: entering delivery always requires a valid acceptance approval.
            if after.state == JobState::Delivery && before.state != JobState::Delivery {
                prop_assert_eq!(cx.approval(GateKind::Acceptance), ApprovalValidity::Valid);
                prop_assert!(matches!(before.state, JobState::AcceptanceApproval | JobState::Blocked), "delivery entered from {:?}", before.state);
            }
            // Acceptance is only ever granted by a human.
            if before.state == JobState::AcceptanceApproval && after.state == JobState::Delivery {
                prop_assert!(matches!(who, Actor::User { .. }), "acceptance granted by a non-user");
            }
            // The Deliver effect is only emitted when entering delivery.
            if d.effects.contains(&Effect::Deliver) {
                prop_assert_eq!(after.state, JobState::Delivery);
            }
            // Blocked state and block info always agree; there is always a way out.
            prop_assert_eq!(after.state == JobState::Blocked, after.blocked.is_some());
            if let Some(info) = &after.blocked {
                prop_assert!(!info.allowed_targets.is_empty());
            }
            // Loop budgets are never exceeded.
            let p = &after.profile;
            prop_assert!(after.counters.fix_iterations <= p.max_fix_iterations);
            prop_assert!(after.counters.back_transitions <= p.max_back_transitions);
            prop_assert!(after.counters.replans <= p.max_replans);
            prop_assert!(after.counters.respecs <= p.max_respecs);
            // The profile snapshot never changes inside the engine.
            prop_assert_eq!(&after.profile, &before.profile);
            // The dependency-wait flag and hold always change together.
            prop_assert_eq!(
                after.ready_for_implementation,
                after.holds.contains(&turtle_workflow::Hold::WaitingForDependencies)
            );
            // No duplicate holds.
            let mut holds = after.holds.clone();
            holds.dedup();
            prop_assert_eq!(holds.len(), after.holds.len());

            job = d.job;
        }
    }
}
