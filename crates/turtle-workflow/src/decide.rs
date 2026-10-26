//! The pure decision function (`docs/WORKFLOW.md` §4, ADR-005).
//!
//! `decide` takes the current job, a command, who issued it and precomputed facts, and
//! returns either a [`Decision`] (new state + events + effects) or a [`Rejection`] naming the
//! guard that failed. It performs no IO and never panics on any input.

use turtle_domain::{Actor, ApprovalValidity, GateKind, JobState, Role};
use turtle_policy::MaterialChange;

use crate::model::{
    BlockInfo, BlockReason, Decision, DecisionContext, Effect, Hold, JobAggregate, JobCommand,
    PauseMode, Rejection, ReviewVerdict, RunPurpose, VerificationOutcome, WorkflowEvent,
};
use crate::profile::ScopeApprovalMode;

use JobState as S;

/// Decides the outcome of `cmd` for `job`.
pub fn decide(
    job: &JobAggregate,
    cmd: &JobCommand,
    actor: &Actor,
    cx: &DecisionContext,
) -> Result<Decision, Rejection> {
    let reject = |guard: &'static str| Rejection {
        state: job.state,
        command: cmd.name().to_owned(),
        guard,
    };

    check_actor(cmd, actor).map_err(reject)?;
    if job.state.is_terminal() {
        return Err(reject("job_not_terminal"));
    }

    let mut tx = Tx {
        job: job.clone(),
        events: Vec::new(),
        effects: Vec::new(),
        cx,
    };
    apply(&mut tx, cmd, actor).map_err(reject)?;
    tx.job.version += 1;
    Ok(Decision {
        job: tx.job,
        events: tx.events,
        effects: tx.effects,
    })
}

// ---------------------------------------------------------------- actor rules

/// Who may issue which command (invariants AP-2 and I-6).
fn check_actor(cmd: &JobCommand, actor: &Actor) -> Result<(), &'static str> {
    use JobCommand as C;
    let user = matches!(actor, Actor::User { .. });
    let machine = matches!(actor, Actor::System | Actor::Scheduler);
    if matches!(actor, Actor::AgentRun { .. }) {
        return Err("agents_cannot_issue_commands");
    }
    match cmd {
        C::ApprovalChangesRequested { .. }
        | C::ApprovalRejected { .. }
        | C::ResolveBlock { .. }
        | C::MarkFailed
        | C::Cancel
        | C::CancelAndDeleteBranch
        | C::Pause { .. }
        | C::Resume => {
            if user {
                Ok(())
            } else {
                Err("user_only")
            }
        }
        C::ApprovalGranted { gate } => {
            if user || (machine && gate.may_auto_approve()) {
                Ok(())
            } else {
                Err("user_decides_approval")
            }
        }
        C::Start | C::SpecProduced { .. } | C::PlanProduced { .. } => Ok(()),
        _ => {
            if machine {
                Ok(())
            } else {
                Err("machine_result_only")
            }
        }
    }
}

// ---------------------------------------------------------------- transaction builder

struct Tx<'a> {
    job: JobAggregate,
    events: Vec<WorkflowEvent>,
    effects: Vec<Effect>,
    cx: &'a DecisionContext,
}

#[derive(Clone, Copy)]
enum Loop {
    Fix,
    Replan,
    Respec,
}

impl Tx<'_> {
    fn enter(&mut self, to: JobState, purpose: RunPurpose, backward: bool) {
        self.end_dependency_wait();
        self.events.push(WorkflowEvent::StageEntered {
            from: self.job.state,
            to,
            backward,
        });
        self.job.state = to;
        self.job.blocked = None;
        let effects = self.entry_effects(to, purpose);
        self.effects.extend(effects);
    }

    fn entry_effects(&self, to: JobState, purpose: RunPurpose) -> Vec<Effect> {
        let run = |role| Effect::StartRun { role, purpose };
        match to {
            S::Backlog | S::Blocked => vec![],
            S::Discovery => vec![run(Role::Discovery)],
            S::Specification => vec![run(Role::SpecWriter)],
            S::ScopeApproval => {
                let auto = self.job.profile.scope_approval == ScopeApprovalMode::AutoIfSmall
                    && self.cx.scope_auto_approvable;
                let mut e = vec![Effect::RequestApproval {
                    gate: GateKind::Scope,
                    auto,
                }];
                if !auto {
                    e.push(Effect::NotifyUser);
                }
                e
            }
            S::Planning => vec![run(Role::Planner)],
            S::PlanApproval => vec![
                Effect::RequestApproval {
                    gate: GateKind::Plan,
                    auto: false,
                },
                Effect::NotifyUser,
            ],
            S::Implementation => match purpose {
                RunPurpose::Fix | RunPurpose::Revise => vec![run(Role::Fixer)],
                RunPurpose::Initial | RunPurpose::Retry => {
                    vec![Effect::CreateWorktree, Effect::DispatchReadyTasks]
                }
            },
            S::BuildTest => vec![Effect::RunGates],
            S::Review => vec![run(Role::Reviewer)],
            S::Verification => vec![Effect::RunVerification],
            S::AcceptanceApproval => vec![
                Effect::RequestApproval {
                    gate: GateKind::Acceptance,
                    auto: false,
                },
                Effect::NotifyUser,
            ],
            S::Delivery => vec![Effect::Deliver],
            S::AwaitingRelease => vec![Effect::CleanupWorktree],
            S::Done => vec![Effect::CleanupWorktree, Effect::NotifyUser],
            S::Cancelled | S::Failed => vec![
                Effect::TerminateRuns,
                Effect::ReleaseReservations,
                Effect::SupersedePendingApprovals,
                Effect::CleanupWorktree,
            ],
        }
    }

    fn block(&mut self, reason: BlockReason) {
        let from = self.job.state;
        let info = BlockInfo {
            reason,
            from,
            allowed_targets: allowed_targets(from),
        };
        self.events.push(WorkflowEvent::StageEntered {
            from,
            to: S::Blocked,
            backward: false,
        });
        self.events
            .push(WorkflowEvent::Blocked { info: info.clone() });
        self.job.state = S::Blocked;
        self.job.blocked = Some(info);
        self.effects
            .extend([Effect::ReleaseReservations, Effect::NotifyUser]);
    }

    /// Autonomous backward transition: consumes loop budgets or blocks the job.
    fn auto_back(&mut self, to: JobState, kind: Loop, purpose: RunPurpose) {
        let p = &self.job.profile;
        let c = self.job.counters;
        let (next, max, reason) = match kind {
            Loop::Fix => (
                c.fix_iterations + 1,
                p.max_fix_iterations,
                BlockReason::FixLimitReached,
            ),
            Loop::Replan => (
                c.replans + 1,
                p.max_replans,
                BlockReason::ReplanLimitReached,
            ),
            Loop::Respec => (
                c.respecs + 1,
                p.max_respecs,
                BlockReason::RespecLimitReached,
            ),
        };
        if next > max {
            return self.block(reason);
        }
        if c.back_transitions + 1 > p.max_back_transitions {
            return self.block(BlockReason::BackTransitionLimitReached);
        }
        let counters = &mut self.job.counters;
        counters.back_transitions += 1;
        match kind {
            Loop::Fix => counters.fix_iterations = next,
            // A new plan or spec gets a fresh fix budget.
            Loop::Replan => {
                counters.replans = next;
                counters.fix_iterations = 0;
            }
            Loop::Respec => {
                counters.respecs = next;
                counters.fix_iterations = 0;
            }
        }
        self.enter(to, purpose, true);
    }

    /// Human-initiated backward transition: does not consume loop budgets.
    fn user_back(&mut self, to: JobState, purpose: RunPurpose) {
        self.enter(to, purpose, true);
    }

    fn add_hold(&mut self, hold: Hold) -> Result<(), &'static str> {
        if self.job.holds.contains(&hold) {
            return Err("hold_not_already_present");
        }
        self.events
            .push(WorkflowEvent::HoldAdded { hold: hold.clone() });
        self.job.holds.push(hold);
        Ok(())
    }

    fn remove_hold(&mut self, pred: impl Fn(&Hold) -> bool) -> Result<(), &'static str> {
        let idx = self.job.holds.iter().position(pred).ok_or("hold_present")?;
        let hold = self.job.holds.remove(idx);
        self.events.push(WorkflowEvent::HoldRemoved { hold });
        Ok(())
    }

    fn invalidate(&mut self, gate: GateKind, change: MaterialChange) {
        self.events
            .push(WorkflowEvent::ApprovalInvalidated { gate, change });
    }

    fn re_request(&mut self, gate: GateKind) {
        self.events
            .push(WorkflowEvent::ApprovalReRequested { gate });
        self.effects
            .push(Effect::RequestApproval { gate, auto: false });
        self.effects.push(Effect::NotifyUser);
    }

    /// Ends a dependency wait: the hold and the ready flag always change together
    /// (invariant: `ready_for_implementation` iff `WaitingForDependencies` is held).
    fn end_dependency_wait(&mut self) {
        self.job.ready_for_implementation = false;
        let _ = self.remove_hold(|h| *h == Hold::WaitingForDependencies);
    }

    fn proceed_to_implementation(&mut self) -> Result<(), &'static str> {
        if self.cx.dependencies_delivered {
            self.job.ready_for_implementation = false;
            self.enter(S::Implementation, RunPurpose::Initial, false);
            Ok(())
        } else {
            self.job.ready_for_implementation = true;
            self.add_hold(Hold::WaitingForDependencies)
        }
    }

    fn require_approval(&self, gate: GateKind) -> Result<(), &'static str> {
        if self.cx.approval(gate) == ApprovalValidity::Valid {
            Ok(())
        } else {
            Err("approval_valid")
        }
    }
}

/// Where a human may send a blocked job (`docs/WORKFLOW.md` §9.2).
#[must_use]
pub fn allowed_targets(from: JobState) -> Vec<JobState> {
    match from {
        S::Backlog | S::Discovery | S::Specification | S::ScopeApproval => vec![S::Specification],
        S::Planning | S::PlanApproval => vec![S::Planning, S::Specification],
        S::Implementation | S::BuildTest | S::Review | S::Verification => {
            vec![S::Implementation, S::Planning, S::Specification]
        }
        S::AcceptanceApproval => vec![S::Implementation, S::Planning],
        S::Delivery => vec![S::Delivery, S::BuildTest, S::Implementation],
        S::AwaitingRelease => vec![S::AwaitingRelease],
        S::Blocked | S::Done | S::Cancelled | S::Failed => vec![],
    }
}

const fn approval_stage(gate: GateKind) -> Option<JobState> {
    match gate {
        GateKind::Scope => Some(S::ScopeApproval),
        GateKind::Plan => Some(S::PlanApproval),
        GateKind::Acceptance => Some(S::AcceptanceApproval),
        _ => None,
    }
}

// ---------------------------------------------------------------- transitions

#[allow(clippy::too_many_lines)] // one arm per transition keeps the table reviewable
fn apply(tx: &mut Tx<'_>, cmd: &JobCommand, actor: &Actor) -> Result<(), &'static str> {
    use JobCommand as C;
    let state = tx.job.state;

    // Commands valid in any non-terminal state.
    match cmd {
        C::Cancel | C::CancelAndDeleteBranch => {
            for hold in std::mem::take(&mut tx.job.holds) {
                tx.events.push(WorkflowEvent::HoldRemoved { hold });
            }
            tx.enter(S::Cancelled, RunPurpose::Initial, false);
            if matches!(cmd, C::CancelAndDeleteBranch) {
                tx.effects.push(Effect::DeleteJobBranch);
            }
            return Ok(());
        }
        C::Pause { mode } => {
            if tx.job.is_paused() {
                return Err("not_already_paused");
            }
            tx.add_hold(Hold::Paused { mode: *mode })?;
            if *mode == PauseMode::Hard {
                tx.effects.push(Effect::TerminateRuns);
            }
            return Ok(());
        }
        C::Resume => return tx.remove_hold(|h| matches!(h, Hold::Paused { .. })),
        C::BudgetExhausted => {
            tx.add_hold(Hold::AwaitingBudget)?;
            tx.effects.push(Effect::RequestApproval {
                gate: GateKind::BudgetIncrease,
                auto: false,
            });
            tx.effects.push(Effect::NotifyUser);
            return Ok(());
        }
        C::BudgetRestored => return tx.remove_hold(|h| *h == Hold::AwaitingBudget),
        C::RateLimited { provider } => {
            return tx.add_hold(Hold::RateLimited {
                provider: provider.clone(),
            });
        }
        C::RateLimitCleared { provider } => {
            return tx
                .remove_hold(|h| matches!(h, Hold::RateLimited { provider: p } if p == provider));
        }
        C::Halt { reason } => {
            if state == S::Blocked {
                return Err("not_blocked");
            }
            tx.block(reason.clone());
            return Ok(());
        }
        C::ConflictDetected => {
            if !state.has_worktree() {
                return Err("job_has_worktree");
            }
            tx.block(BlockReason::ConflictDetected);
            return Ok(());
        }
        C::ResolveBlock { target } => {
            let info = tx.job.blocked.as_ref().ok_or("job_blocked")?;
            if !info.allowed_targets.contains(target) {
                return Err("target_allowed");
            }
            // Re-entering a stage never bypasses the approval that normally guards it
            // (invariants J-4 and J-5).
            match target {
                S::Delivery => tx.require_approval(GateKind::Acceptance)?,
                S::Planning => tx.require_approval(GateKind::Scope)?,
                _ => {}
            }
            // A human decision grants a fresh fix budget.
            tx.job.counters.fix_iterations = 0;
            let purpose = if info.reason == BlockReason::ReviewDecisionRequired
                && *target == S::Implementation
            {
                RunPurpose::Fix
            } else {
                RunPurpose::Retry
            };
            tx.enter(*target, purpose, true);
            return Ok(());
        }
        C::MarkFailed => {
            if state != S::Blocked {
                return Err("job_blocked");
            }
            tx.enter(S::Failed, RunPurpose::Initial, false);
            return Ok(());
        }
        C::ApprovalInvalidated { gate, change } => {
            tx.invalidate(*gate, *change);
            if approval_stage(*gate) == Some(state) {
                tx.re_request(*gate);
            }
            return Ok(());
        }
        C::ScopeChangeDetected => {
            if !matches!(
                state,
                S::Implementation | S::BuildTest | S::Review | S::Verification
            ) {
                return Err("stage_accepts_scope_change");
            }
            tx.invalidate(GateKind::Scope, MaterialChange::SpecRevised);
            tx.auto_back(S::Specification, Loop::Respec, RunPurpose::Revise);
            return Ok(());
        }
        _ => {}
    }

    // Approval decisions must match the approval stage the job is in.
    if let C::ApprovalGranted { gate }
    | C::ApprovalChangesRequested { gate }
    | C::ApprovalRejected { gate } = cmd
    {
        match approval_stage(*gate) {
            Some(stage) if stage == state => {}
            _ if !state.is_approval() => return Err("stage_awaits_approval"),
            _ => return Err("gate_matches_stage"),
        }
    }

    match (state, cmd) {
        (S::Backlog, C::Start) => {
            let to = if tx.job.profile.discovery_enabled(tx.job.complexity) {
                S::Discovery
            } else {
                S::Specification
            };
            tx.enter(to, RunPurpose::Initial, false);
        }

        (S::Discovery, C::DiscoveryCompleted { report_valid }) => {
            if !report_valid {
                return Err("report_valid");
            }
            tx.enter(S::Specification, RunPurpose::Initial, false);
        }

        (
            S::Specification,
            C::SpecProduced {
                valid,
                criteria_count,
            },
        ) => {
            check_spec(*valid, *criteria_count)?;
            match tx.cx.preflight {
                None => return Err("preflight_computed"),
                Some(crate::model::PreflightDecision::Deny) => {
                    tx.block(BlockReason::PreflightDenied);
                }
                Some(_) => tx.enter(S::ScopeApproval, RunPurpose::Initial, false),
            }
        }
        // User edits the spec while it awaits approval: the old approval subject is gone.
        (
            S::ScopeApproval,
            C::SpecProduced {
                valid,
                criteria_count,
            },
        ) => {
            check_spec(*valid, *criteria_count)?;
            tx.invalidate(GateKind::Scope, MaterialChange::SpecRevised);
            tx.re_request(GateKind::Scope);
        }
        (S::ScopeApproval, C::ApprovalGranted { .. }) => {
            if !matches!(actor, Actor::User { .. })
                && !(tx.job.profile.scope_approval == ScopeApprovalMode::AutoIfSmall
                    && tx.cx.scope_auto_approvable)
            {
                return Err("auto_scope_allowed");
            }
            tx.require_approval(GateKind::Scope)?;
            if tx.cx.preflight == Some(crate::model::PreflightDecision::Deny) {
                return Err("preflight_not_denied");
            }
            tx.enter(S::Planning, RunPurpose::Initial, false);
        }
        (S::ScopeApproval, C::ApprovalChangesRequested { .. }) => {
            tx.user_back(S::Specification, RunPurpose::Revise);
        }

        (
            S::Planning,
            C::PlanProduced {
                valid,
                within_scope,
            },
        ) => {
            if !valid {
                return Err("plan_valid");
            }
            tx.end_dependency_wait();
            if !within_scope {
                tx.invalidate(GateKind::Scope, MaterialChange::PlanRevised);
                tx.auto_back(S::Specification, Loop::Respec, RunPurpose::Revise);
            } else if tx
                .job
                .profile
                .plan_approval_required(tx.cx.plan_thresholds_exceeded)
            {
                tx.enter(S::PlanApproval, RunPurpose::Initial, false);
            } else {
                tx.proceed_to_implementation()?;
            }
        }
        (S::PlanApproval, C::PlanProduced { valid, .. }) => {
            if !valid {
                return Err("plan_valid");
            }
            tx.end_dependency_wait();
            tx.invalidate(GateKind::Plan, MaterialChange::PlanRevised);
            tx.re_request(GateKind::Plan);
        }
        (S::PlanApproval, C::ApprovalGranted { .. }) => {
            tx.require_approval(GateKind::Plan)?;
            tx.proceed_to_implementation()?;
        }
        (S::PlanApproval, C::ApprovalChangesRequested { .. }) => {
            tx.user_back(S::Planning, RunPurpose::Revise);
        }
        (S::Planning | S::PlanApproval, C::DependenciesDelivered) => {
            if !tx.job.ready_for_implementation {
                return Err("ready_for_implementation");
            }
            tx.enter(S::Implementation, RunPurpose::Initial, false);
        }

        (S::Implementation, C::AllTasksSucceeded) => {
            tx.enter(S::BuildTest, RunPurpose::Initial, false);
        }
        (S::Implementation, C::TaskFailed { replan_allowed }) => {
            if *replan_allowed {
                tx.auto_back(S::Planning, Loop::Replan, RunPurpose::Revise);
            } else {
                tx.block(BlockReason::FixLimitReached);
            }
        }

        (
            S::BuildTest,
            C::GatesFinished {
                all_required_passed,
            },
        ) => {
            if *all_required_passed {
                tx.enter(S::Review, RunPurpose::Initial, false);
            } else {
                tx.auto_back(S::Implementation, Loop::Fix, RunPurpose::Fix);
            }
        }

        (S::Review, C::ReviewCompleted { verdict }) => match verdict {
            ReviewVerdict::Pass => tx.enter(S::Verification, RunPurpose::Initial, false),
            ReviewVerdict::Fixable => {
                tx.auto_back(S::Implementation, Loop::Fix, RunPurpose::Fix);
            }
            ReviewVerdict::Replan => tx.auto_back(S::Planning, Loop::Replan, RunPurpose::Revise),
            ReviewVerdict::ScopeCreep => {
                tx.invalidate(GateKind::Scope, MaterialChange::SpecRevised);
                tx.auto_back(S::Specification, Loop::Respec, RunPurpose::Revise);
            }
        },

        (S::Verification, C::VerificationCompleted { outcome }) => match outcome {
            VerificationOutcome::Met => {
                tx.enter(S::AcceptanceApproval, RunPurpose::Initial, false);
            }
            VerificationOutcome::NotMetFixable => {
                tx.auto_back(S::Implementation, Loop::Fix, RunPurpose::Fix);
            }
            VerificationOutcome::NotMetStructural => {
                tx.auto_back(S::Planning, Loop::Replan, RunPurpose::Revise);
            }
        },

        (S::AcceptanceApproval, C::ApprovalGranted { .. }) => {
            tx.require_approval(GateKind::Acceptance)?;
            if !tx.cx.flags_acknowledged {
                return Err("flags_acknowledged");
            }
            tx.enter(S::Delivery, RunPurpose::Initial, false);
        }
        (S::AcceptanceApproval, C::ApprovalChangesRequested { .. }) => {
            tx.user_back(S::Implementation, RunPurpose::Revise);
        }

        (S::Delivery, C::Delivered) => {
            let to = if tx.job.profile.release_required {
                S::AwaitingRelease
            } else {
                S::Done
            };
            tx.enter(to, RunPurpose::Initial, false);
        }
        (S::Delivery, C::BaseRefreshed) => {
            tx.invalidate(GateKind::Acceptance, MaterialChange::BaseRefreshed);
            tx.enter(S::BuildTest, RunPurpose::Initial, true);
        }

        (S::AwaitingRelease, C::ReleaseCompleted) => {
            tx.enter(S::Done, RunPurpose::Initial, false);
        }

        // Rejection is terminal for the job.
        (_, C::ApprovalRejected { .. }) => {
            tx.enter(S::Cancelled, RunPurpose::Initial, false);
        }

        _ => return Err("no_transition"),
    }
    Ok(())
}

fn check_spec(valid: bool, criteria_count: u32) -> Result<(), &'static str> {
    if !valid {
        Err("spec_valid")
    } else if criteria_count == 0 {
        Err("criteria_nonempty")
    } else {
        Ok(())
    }
}
