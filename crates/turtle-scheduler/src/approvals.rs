//! Requesting and deciding approvals. This is the only path from a human decision to a job
//! transition: the decision is checked against the *current* state, recorded, and only then
//! submitted to the engine, whose guard recomputes validity once more.

use std::sync::Arc;

use turtle_domain::{Actor, ApprovalId, GateKind, Sha256};
use turtle_store::{ApprovalRecord, BudgetRecord, BudgetScope, Decision, JobRecord, StoreError};
use turtle_workflow::{DecisionContext, JobCommand, decide};

use crate::binding::{
    BindingError, GitFactsProvider, POLICY_VERSION, build_binding, build_budget_binding,
};
use crate::context::read_snapshot;
use crate::engine::{ContextProvider, Engine, EngineError};

#[derive(Debug, thiserror::Error)]
pub enum ApprovalError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Engine(#[from] EngineError),
    #[error(transparent)]
    Binding(#[from] BindingError),
    /// The thing being approved changed after the approval was requested. The approval is
    /// invalidated and must be requested again.
    #[error("what this approval covers has changed since it was requested; request it again")]
    Stale,
    #[error("delivery needs a current approved {0:?} approval")]
    MissingCurrent(GateKind),
    #[error("these flags must be acknowledged before approving: {}", .0.join(", "))]
    FlagsNotAcknowledged(Vec<String>),
    #[error("{0}")]
    InvalidBudget(String),
}

fn validate_budget(
    budgets: &[BudgetRecord],
    job: turtle_domain::JobId,
    requested: u64,
) -> Result<BudgetRecord, ApprovalError> {
    let current = budgets
        .iter()
        .find(|b| b.scope == BudgetScope::Job(job) && b.hard)
        .ok_or_else(|| ApprovalError::InvalidBudget("the job has no hard token budget".into()))?;
    if requested <= current.limit_tokens {
        return Err(ApprovalError::InvalidBudget(
            "the new limit must exceed the current limit".into(),
        ));
    }
    if requested > i64::MAX as u64 {
        return Err(ApprovalError::InvalidBudget(
            "the limit is too large".into(),
        ));
    }
    if budgets
        .iter()
        .any(|b| b.hard && b.scope != BudgetScope::Job(job) && requested > b.limit_tokens)
    {
        return Err(ApprovalError::InvalidBudget(
            "the requested limit exceeds a configured hard project or global ceiling".into(),
        ));
    }
    if budgets.iter().any(|b| {
        b.hard && b.scope != BudgetScope::Job(job) && b.headroom() < crate::budget::MIN_HEADROOM
    }) {
        return Err(ApprovalError::InvalidBudget(
            "a project or global hard budget also needs more headroom".into(),
        ));
    }
    if requested.saturating_sub(
        current
            .settled_tokens
            .saturating_add(current.reserved_tokens),
    ) < crate::budget::MIN_HEADROOM
    {
        return Err(ApprovalError::InvalidBudget(
            "the new limit needs at least 10,000 tokens of headroom".into(),
        ));
    }
    Ok(current.clone())
}

pub struct ApprovalService<C, G> {
    engine: Engine<C>,
    git: Arc<G>,
    policy_version: u32,
}

impl<C, G> Clone for ApprovalService<C, G> {
    fn clone(&self) -> Self {
        Self {
            engine: self.engine.clone(),
            git: Arc::clone(&self.git),
            policy_version: self.policy_version,
        }
    }
}

impl<C, G> std::fmt::Debug for ApprovalService<C, G> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApprovalService").finish_non_exhaustive()
    }
}

impl<C: ContextProvider, G: GitFactsProvider> ApprovalService<C, G> {
    #[must_use]
    pub fn new(engine: Engine<C>, git: Arc<G>) -> Self {
        Self {
            engine,
            git,
            policy_version: POLICY_VERSION,
        }
    }

    async fn current_binding(
        &self,
        job: &JobRecord,
        gate: GateKind,
    ) -> Result<turtle_policy::ApprovalBinding, ApprovalError> {
        let snap = read_snapshot(self.engine.store(), job).await?;
        let git = self.git.facts(job).await;
        Ok(build_binding(
            job,
            gate,
            &snap.artifacts,
            git.as_ref(),
            self.policy_version,
            snap.job_budget_tokens,
        )?)
    }

    /// Creates a pending approval for the exact increase the user selected.
    pub async fn request_budget_increase(
        &self,
        job_id: turtle_domain::JobId,
        requested: u64,
    ) -> Result<ApprovalRecord, ApprovalError> {
        let version = self.policy_version;
        let result = self
            .engine
            .store()
            .with(move |s| {
                s.transaction(|tx| {
                    let job = tx.load_job(job_id)?;
                    let budgets = tx.budgets_for_job(job_id)?;
                    let budget = validate_budget(&budgets, job_id, requested)
                        .map_err(|e| StoreError::Forbidden(e.to_string()))?;
                    let binding = build_budget_binding(&job, &budget, requested, version)
                        .map_err(|e| StoreError::Forbidden(e.to_string()))?;
                    tx.request_approval(&binding).map(|(approval, _)| approval)
                })
            })
            .await;
        result.map_err(|e| match e {
            StoreError::Forbidden(message) => ApprovalError::InvalidBudget(message),
            other => other.into(),
        })
    }

    /// Rechecks the approval against current artifacts and Git immediately before delivery.
    pub async fn current_approved(
        &self,
        job: &JobRecord,
        gate: GateKind,
    ) -> Result<ApprovalRecord, ApprovalError> {
        let id = job.job.id;
        let approved = self
            .engine
            .store()
            .with(move |s| {
                let approved = s.latest_approved(id, gate)?;
                if let turtle_store::ChainVerdict::Broken { seq, .. } = s.verify_chain()? {
                    return Err(StoreError::Corrupt(format!(
                        "audit chain broken at event {seq}"
                    )));
                }
                Ok(approved)
            })
            .await?
            .ok_or(ApprovalError::MissingCurrent(gate))?;
        if approved.binding.subject_hash() != approved.subject_hash
            || approved.binding.gate_kind != gate
            || approved.binding.job_id != id
            || self.current_binding(job, gate).await?.subject_hash() != approved.subject_hash
        {
            return Err(ApprovalError::Stale);
        }
        Ok(approved)
    }

    /// Requests an approval for the job's current state. Idempotent: asking again while nothing
    /// changed returns the same pending approval; a changed subject supersedes the old one.
    pub async fn request(
        &self,
        job_id: turtle_domain::JobId,
        gate: GateKind,
    ) -> Result<(ApprovalRecord, bool), ApprovalError> {
        let job = self
            .engine
            .store()
            .with(move |s| s.load_job(job_id))
            .await?;
        let binding = self.current_binding(&job, gate).await?;
        Ok(self
            .engine
            .store()
            .with(move |s| s.transaction(|tx| tx.request_approval(&binding)))
            .await?)
    }

    /// Records a human decision and applies it to the job.
    ///
    /// `echoed_hash` is the hash the human was shown. For approvals, `acknowledged` must cover
    /// every flag in the binding (protected paths, dependency changes…).
    #[allow(clippy::too_many_arguments)]
    pub async fn decide(
        &self,
        id: ApprovalId,
        decision: Decision,
        echoed_hash: Sha256,
        actor: Actor,
        channel: &str,
        comment: Option<String>,
        acknowledged: Vec<String>,
    ) -> Result<JobRecord, ApprovalError> {
        let approval = self.engine.store().with(move |s| s.approval(id)).await?;
        let job_id = approval.job_id;
        let gate = approval.gate;

        // The human decides on what they saw; make sure it is still what exists.
        if approval.status == turtle_domain::ApprovalStatus::Pending {
            let job = self
                .engine
                .store()
                .with(move |s| s.load_job(job_id))
                .await?;
            let current = if gate == GateKind::BudgetIncrease {
                let requested = approval
                    .binding
                    .budget_increase
                    .as_ref()
                    .map(|b| b.requested_tokens);
                let budget = self
                    .engine
                    .store()
                    .with(move |s| s.transaction(|tx| tx.budgets_for_job(job_id)))
                    .await?;
                match requested.and_then(|requested| {
                    validate_budget(&budget, job_id, requested)
                        .ok()
                        .map(|budget| (requested, budget))
                }) {
                    Some((requested, budget)) => {
                        build_budget_binding(&job, &budget, requested, self.policy_version)
                            .map_err(ApprovalError::from)
                    }
                    None => Err(ApprovalError::Stale),
                }
            } else {
                self.current_binding(&job, gate).await
            };
            if current.map(|b| b.subject_hash()).ok() != Some(approval.subject_hash) {
                self.engine
                    .store()
                    .with(move |s| {
                        s.transaction(|tx| {
                            tx.invalidate_approvals(job_id, &[gate], "stale_at_decision")
                        })
                    })
                    .await?;
                return Err(ApprovalError::Stale);
            }
        }
        if decision == Decision::Approve {
            let missing: Vec<String> = approval
                .binding
                .acknowledged_flags
                .iter()
                .filter(|f| !acknowledged.contains(f))
                .cloned()
                .collect();
            if !missing.is_empty() {
                return Err(ApprovalError::FlagsNotAcknowledged(missing));
            }
        }

        if gate == GateKind::BudgetIncrease {
            if decision != Decision::Approve {
                return Err(ApprovalError::InvalidBudget(
                    "budget increases can only be approved; choose a new amount instead".into(),
                ));
            }
            let bound = approval
                .binding
                .budget_increase
                .as_ref()
                .ok_or(ApprovalError::Stale)?;
            let requested = bound.requested_tokens;
            let policy_version = self.policy_version;
            let who = actor.clone();
            let ch = channel.to_owned();
            let result = self
                .engine
                .store()
                .with(move |s| {
                    s.transaction(|tx| {
                        let job = tx.load_job(job_id)?;
                        let budgets = tx.budgets_for_job(job_id)?;
                        let budget = validate_budget(&budgets, job_id, requested)
                            .map_err(|_| StoreError::Forbidden("stale budget approval".into()))?;
                        let expected =
                            build_budget_binding(&job, &budget, requested, policy_version)
                                .map_err(|_| {
                                    StoreError::Forbidden("stale budget approval".into())
                                })?;
                        if expected.subject_hash() != echoed_hash
                            || approval.binding.subject_hash() != echoed_hash
                        {
                            return Err(StoreError::Forbidden("stale budget approval".into()));
                        }
                        let command = JobCommand::BudgetIncreaseApproved;
                        let transition =
                            decide(&job.job, &command, &who, &DecisionContext::default())
                                .map_err(|e| StoreError::Forbidden(e.to_string()))?;
                        tx.decide_approval(
                            id,
                            Decision::Approve,
                            &echoed_hash,
                            &who,
                            &ch,
                            comment.as_deref(),
                        )?;
                        tx.set_budget(
                            BudgetScope::Job(job_id),
                            requested,
                            true,
                            Some(job_id),
                            &who,
                        )?;
                        tx.apply_decision(job.job.version, &transition, &command, &who)?;
                        tx.load_job(job_id)
                    })
                })
                .await;
            return result.map_err(|e| match e {
                StoreError::Forbidden(message) if message == "stale budget approval" => {
                    ApprovalError::Stale
                }
                other => other.into(),
            });
        }

        let (who, ch) = (actor.clone(), channel.to_owned());
        self.engine
            .store()
            .with(move |s| {
                s.transaction(|tx| {
                    tx.decide_approval(id, decision, &echoed_hash, &who, &ch, comment.as_deref())
                })
            })
            .await?;

        let command = match decision {
            Decision::Approve => JobCommand::ApprovalGranted { gate },
            Decision::RequestChanges => JobCommand::ApprovalChangesRequested { gate },
            Decision::Reject => JobCommand::ApprovalRejected { gate },
        };
        Ok(self.engine.submit(job_id, command, actor).await?)
    }
}
