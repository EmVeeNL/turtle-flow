//! The command → decision → transaction loop (`docs/ARCHITECTURE.md` §3.2).
//!
//! Every state change is: load the job, compute the facts, run the pure `decide()`, and persist
//! the decision atomically. When the job changed in between (version conflict) the whole thing
//! is retried on fresh state, so a decision is never applied to a state it was not made for.

use std::future::Future;
use std::sync::Arc;

use serde_json::{Value, json};
use turtle_domain::{Actor, JobId};
use turtle_store::{EffectFate, EffectRow, JobRecord, RecoveryReport, StoreError};
use turtle_workflow::{BlockReason, DecisionContext, JobCommand, Rejection, decide};

use crate::crash::{CrashPlan, Crashed};
use crate::shared::SharedStore;

const MAX_CONFLICT_RETRIES: usize = 8;

/// Supplies the facts `decide()` needs (approval validity, preflight, dependencies…).
/// Computed outside the transaction because it may do IO.
pub trait ContextProvider: Send + Sync + 'static {
    fn context(&self, job: &JobRecord) -> impl Future<Output = DecisionContext> + Send;
}

/// A fixed context, used in tests and by callers that precompute everything.
#[derive(Debug, Clone, Default)]
pub struct StaticContext(pub DecisionContext);

impl ContextProvider for StaticContext {
    async fn context(&self, _job: &JobRecord) -> DecisionContext {
        self.0.clone()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Rejected(#[from] Rejection),
    #[error("job kept changing underneath the command ({MAX_CONFLICT_RETRIES} attempts)")]
    TooManyConflicts,
    #[error(transparent)]
    Crashed(#[from] Crashed),
}

/// How an effect ended up after the executor reported its outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Finished {
    Completed,
    /// The lease was gone (restart, recovery); nothing was written.
    LeaseLost,
    Requeued,
    Failed,
}

pub struct Engine<C> {
    pub(crate) store: SharedStore,
    ctx: Arc<C>,
    crash: CrashPlan,
}

impl<C> Clone for Engine<C> {
    fn clone(&self) -> Self {
        Self {
            store: self.store.clone(),
            ctx: Arc::clone(&self.ctx),
            crash: self.crash.clone(),
        }
    }
}

impl<C> std::fmt::Debug for Engine<C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Engine").finish_non_exhaustive()
    }
}

impl<C: ContextProvider> Engine<C> {
    #[must_use]
    pub fn new(store: SharedStore, ctx: Arc<C>, crash: CrashPlan) -> Self {
        Self { store, ctx, crash }
    }

    #[must_use]
    pub fn store(&self) -> &SharedStore {
        &self.store
    }

    /// Applies one command from `actor` and returns the job as stored afterwards.
    pub async fn submit(
        &self,
        job: JobId,
        cmd: JobCommand,
        actor: Actor,
    ) -> Result<JobRecord, EngineError> {
        for _ in 0..MAX_CONFLICT_RETRIES {
            let record = self.store.with(move |s| s.load_job(job)).await?;
            let cx = self.ctx.context(&record).await;
            let decision = decide(&record.job, &cmd, &actor, &cx)?;
            self.crash.point("submit:decided")?;
            let (c, a, version) = (cmd.clone(), actor.clone(), record.job.version);
            let stored = self
                .store
                .with(move |s| {
                    s.transaction(|tx| {
                        tx.apply_decision(version, &decision, &c, &a)?;
                        tx.load_job(job)
                    })
                })
                .await;
            match stored {
                Ok(rec) => {
                    self.crash.point("submit:committed")?;
                    return Ok(rec);
                }
                Err(StoreError::VersionConflict { .. }) => {}
                Err(e) => return Err(e.into()),
            }
        }
        Err(EngineError::TooManyConflicts)
    }

    /// Completes an effect and, in the **same transaction**, applies the command its execution
    /// produced. A crash can therefore never complete an effect without applying its result, or
    /// apply the result without completing the effect.
    pub async fn finish_effect(
        &self,
        effect: &EffectRow,
        instance: &str,
        result: Value,
        command: Option<JobCommand>,
    ) -> Result<Finished, EngineError> {
        for _ in 0..MAX_CONFLICT_RETRIES {
            let (id, inst, res) = (effect.id, instance.to_owned(), result.clone());
            let (Some(job_id), Some(cmd)) = (effect.job_id, command.clone()) else {
                return self.complete_only(id, inst, res).await;
            };

            let record = self.store.with(move |s| s.load_job(job_id)).await?;
            let cx = self.ctx.context(&record).await;
            let actor = Actor::Scheduler;
            let decision = match decide(&record.job, &cmd, &actor, &cx) {
                Ok(d) => d,
                Err(rejected) => {
                    // The world moved on (cancelled, paused, superseded): drop the stale result
                    // deterministically instead of failing the effect.
                    let note = json!({ "result": res, "command_rejected": rejected.guard });
                    return self.complete_only(id, inst, note).await;
                }
            };
            self.crash.point("finish:decided")?;
            let version = record.job.version;
            let stored = self
                .store
                .with(move |s| {
                    s.transaction(|tx| {
                        tx.complete_effect(id, &inst, &res)?;
                        tx.apply_decision(version, &decision, &cmd, &actor)?;
                        Ok(())
                    })
                })
                .await;
            match stored {
                Ok(()) => {
                    self.crash.point("finish:committed")?;
                    return Ok(Finished::Completed);
                }
                Err(StoreError::LeaseLost { .. }) => return Ok(Finished::LeaseLost),
                Err(StoreError::VersionConflict { .. }) => {}
                Err(e) => return Err(e.into()),
            }
        }
        Err(EngineError::TooManyConflicts)
    }

    async fn complete_only(
        &self,
        id: turtle_domain::EffectId,
        instance: String,
        result: Value,
    ) -> Result<Finished, EngineError> {
        let r = self
            .store
            .with(move |s| s.transaction(|tx| tx.complete_effect(id, &instance, &result)))
            .await;
        match r {
            Ok(()) => {
                self.crash.point("finish:committed")?;
                Ok(Finished::Completed)
            }
            Err(StoreError::LeaseLost { .. }) => Ok(Finished::LeaseLost),
            Err(e) => Err(e.into()),
        }
    }

    /// Applies a hold command (such as `BudgetExhausted`) and returns the effect to the queue,
    /// in one transaction. A command the workflow rejects (the hold is already there) is not
    /// an error: the effect is simply deferred.
    pub async fn defer_effect(
        &self,
        effect: &EffectRow,
        instance: &str,
        command: JobCommand,
        reason: &str,
    ) -> Result<Finished, EngineError> {
        self.defer_effect_inner(effect, instance, command, reason, None)
            .await
    }

    pub async fn defer_effect_until(
        &self,
        effect: &EffectRow,
        instance: &str,
        command: JobCommand,
        reason: &str,
        not_before: &str,
    ) -> Result<Finished, EngineError> {
        self.defer_effect_inner(
            effect,
            instance,
            command,
            reason,
            Some(not_before.to_owned()),
        )
        .await
    }

    async fn defer_effect_inner(
        &self,
        effect: &EffectRow,
        instance: &str,
        command: JobCommand,
        reason: &str,
        not_before: Option<String>,
    ) -> Result<Finished, EngineError> {
        for _ in 0..MAX_CONFLICT_RETRIES {
            let applied = match effect.job_id {
                Some(job_id) => {
                    let record = self.store.with(move |s| s.load_job(job_id)).await?;
                    let cx = self.ctx.context(&record).await;
                    decide(&record.job, &command, &Actor::Scheduler, &cx)
                        .ok()
                        .map(|d| (record.job.version, d))
                }
                None => None,
            };
            let (id, inst, why, cmd, due) = (
                effect.id,
                instance.to_owned(),
                reason.to_owned(),
                command.clone(),
                not_before.clone(),
            );
            let outcome = self
                .store
                .with(move |s| {
                    s.transaction(|tx| {
                        if let Some((version, decision)) = &applied {
                            tx.apply_decision(*version, decision, &cmd, &Actor::Scheduler)?;
                        }
                        tx.defer_effect(id, &inst, &why, due.as_deref())
                    })
                })
                .await;
            match outcome {
                Ok(()) => return Ok(Finished::Requeued),
                Err(StoreError::LeaseLost { .. }) => return Ok(Finished::LeaseLost),
                Err(StoreError::VersionConflict { .. }) => {}
                Err(e) => return Err(e.into()),
            }
        }
        Err(EngineError::TooManyConflicts)
    }

    /// Records a failed attempt. A permanent failure (non-retryable, or attempts exhausted)
    /// also tells the workflow, in the same transaction, so the job blocks instead of waiting
    /// forever for an effect that will never finish.
    pub async fn fail_effect(
        &self,
        effect: &EffectRow,
        instance: &str,
        message: &str,
        retry_in: Option<std::time::Duration>,
    ) -> Result<Finished, EngineError> {
        let permanent = retry_in.is_none() || effect.attempts >= effect.max_attempts;
        for _ in 0..MAX_CONFLICT_RETRIES {
            let (id, inst, msg) = (effect.id, instance.to_owned(), message.to_owned());
            let halt = match (permanent, effect.job_id) {
                (true, Some(job_id)) => self.halt_decision(job_id, effect).await?,
                _ => None,
            };
            let outcome = self
                .store
                .with(move |s| {
                    s.transaction(|tx| {
                        let fate = tx.fail_effect(id, &inst, &msg, retry_in)?;
                        if let Some((version, decision, cmd)) = &halt
                            && fate == EffectFate::Failed
                        {
                            tx.apply_decision(*version, decision, cmd, &Actor::Scheduler)?;
                        }
                        Ok(fate)
                    })
                })
                .await;
            match outcome {
                Ok(EffectFate::Requeued) => return Ok(Finished::Requeued),
                Ok(EffectFate::Failed) => return Ok(Finished::Failed),
                Err(StoreError::LeaseLost { .. }) => return Ok(Finished::LeaseLost),
                Err(StoreError::VersionConflict { .. }) => {}
                Err(e) => return Err(e.into()),
            }
        }
        Err(EngineError::TooManyConflicts)
    }

    #[allow(clippy::type_complexity)]
    async fn halt_decision(
        &self,
        job_id: JobId,
        effect: &EffectRow,
    ) -> Result<Option<(u64, turtle_workflow::Decision, JobCommand)>, EngineError> {
        let record = self.store.with(move |s| s.load_job(job_id)).await?;
        let cx = self.ctx.context(&record).await;
        let cmd = JobCommand::Halt {
            reason: BlockReason::Unrecoverable {
                code: format!("effect_failed:{}", effect.kind),
            },
        };
        // Already blocked, finished or otherwise unable to halt: just fail the effect.
        Ok(decide(&record.job, &cmd, &Actor::Scheduler, &cx)
            .ok()
            .map(|d| (record.job.version, d, cmd)))
    }

    /// Startup recovery: requeue effects abandoned by a dead instance and block jobs whose
    /// effects had already used every attempt.
    pub async fn recover(&self, instance: &str) -> Result<RecoveryReport, EngineError> {
        let inst = instance.to_owned();
        let report = self
            .store
            .with(move |s| s.transaction(|tx| tx.recover_after_restart(&inst)))
            .await?;
        for failed in &report.effects_failed {
            let Some(job_id) = failed.job_id else {
                continue;
            };
            let cmd = JobCommand::Halt {
                reason: BlockReason::Unrecoverable {
                    code: format!("effect_failed:{}", failed.kind),
                },
            };
            match self.submit(job_id, cmd, Actor::Scheduler).await {
                Ok(_) | Err(EngineError::Rejected(_)) => {}
                Err(e) => return Err(e),
            }
        }
        Ok(report)
    }
}
