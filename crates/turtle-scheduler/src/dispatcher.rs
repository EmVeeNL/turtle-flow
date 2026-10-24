//! The effect dispatcher: claims effects with leases, runs them through an [`Executor`] with
//! heartbeats, and reports outcomes through the [`Engine`] (`docs/adr/ADR-011`, `ADR-017`).
//!
//! Delivery is *at-least-once*: a crash after a side effect but before its completion makes the
//! effect run again after recovery. Executors must therefore probe the real world first (the
//! effect carries a deterministic idempotency key) and treat "already done" as success.

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use tokio::sync::{Notify, watch};
use tokio::task::JoinSet;
use turtle_store::{ClaimParams, EffectRow, RecoveryReport, StoreError};
use turtle_workflow::{Effect, JobCommand};

use crate::crash::{CrashPlan, Crashed};
use crate::engine::{ContextProvider, Engine, EngineError, Finished};
use crate::shared::SharedStore;

#[derive(Debug, Clone)]
pub struct DispatcherConfig {
    pub lease_ttl: Duration,
    /// How often a running effect renews its lease; must be well below `lease_ttl`.
    pub heartbeat: Duration,
    /// Maximum effects running at once (global concurrency limit).
    pub max_concurrent: u32,
    pub poll_interval: Duration,
}

impl Default for DispatcherConfig {
    fn default() -> Self {
        Self {
            lease_ttl: Duration::from_secs(30),
            heartbeat: Duration::from_secs(10),
            max_concurrent: 3,
            poll_interval: Duration::from_secs(2),
        }
    }
}

/// What a successful execution produced.
#[derive(Debug, Clone, Default)]
pub struct ExecOutcome {
    pub result: Value,
    /// Fed to the workflow in the same transaction that completes the effect.
    pub command: Option<JobCommand>,
}

impl ExecOutcome {
    #[must_use]
    pub fn done(result: Value) -> Self {
        Self {
            result,
            command: None,
        }
    }

    #[must_use]
    pub fn with_command(mut self, command: JobCommand) -> Self {
        self.command = Some(command);
        self
    }
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum ExecError {
    /// Transient: retried with backoff until attempts run out.
    #[error("{0}")]
    Retryable(String),
    /// Permanent: the effect fails now and the job is blocked.
    #[error("{0}")]
    Fatal(String),
}

/// Performs effects. Must be idempotent per `effect.idempotency_key`.
pub trait Executor: Send + Sync + 'static {
    fn execute(
        &self,
        effect: &EffectRow,
    ) -> impl Future<Output = Result<ExecOutcome, ExecError>> + Send;
}

#[derive(Debug, thiserror::Error)]
pub enum DispatchError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Engine(#[from] EngineError),
    #[error(transparent)]
    Crashed(#[from] Crashed),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TickReport {
    pub claimed: u32,
    pub completed: u32,
    pub requeued: u32,
    pub failed: u32,
    pub lease_lost: u32,
}

/// Exponential backoff for retryable failures: 10 s, 20 s, 40 s, 80 s, then capped at 2 min.
#[must_use]
pub fn backoff(attempt: u32) -> Duration {
    let exp = attempt.saturating_sub(1).min(4);
    Duration::from_secs(10 * 2u64.pow(exp)).min(Duration::from_secs(120))
}

pub struct Dispatcher<E, C> {
    engine: Engine<C>,
    executor: Arc<E>,
    config: DispatcherConfig,
    instance: String,
    crash: CrashPlan,
    wake: Arc<Notify>,
}

impl<E, C> Clone for Dispatcher<E, C> {
    fn clone(&self) -> Self {
        Self {
            engine: self.engine.clone(),
            executor: Arc::clone(&self.executor),
            config: self.config.clone(),
            instance: self.instance.clone(),
            crash: self.crash.clone(),
            wake: Arc::clone(&self.wake),
        }
    }
}

impl<E, C> std::fmt::Debug for Dispatcher<E, C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Dispatcher")
            .field("instance", &self.instance)
            .finish_non_exhaustive()
    }
}

impl<E: Executor, C: ContextProvider> Dispatcher<E, C> {
    /// Registers this process as a new daemon instance and runs startup recovery before any
    /// effect is claimed.
    pub async fn start(
        store: SharedStore,
        ctx: Arc<C>,
        executor: Arc<E>,
        config: DispatcherConfig,
        crash: CrashPlan,
    ) -> Result<(Self, RecoveryReport), DispatchError> {
        let instance = store
            .with(|s| {
                s.transaction(|tx| {
                    tx.register_instance(env!("CARGO_PKG_VERSION"), std::process::id())
                })
            })
            .await?;
        let engine = Engine::new(store, ctx, crash.clone());
        let report = engine.recover(&instance).await?;
        Ok((
            Self {
                engine,
                executor,
                config,
                instance,
                crash,
                wake: Arc::new(Notify::new()),
            },
            report,
        ))
    }

    #[must_use]
    pub fn instance_id(&self) -> &str {
        &self.instance
    }

    #[must_use]
    pub fn engine(&self) -> &Engine<C> {
        &self.engine
    }

    /// Wakes the loop immediately (call after submitting a command).
    pub fn wake(&self) {
        self.wake.notify_one();
    }

    /// Marks this instance as cleanly stopped.
    pub async fn shutdown(&self) -> Result<(), DispatchError> {
        let id = self.instance.clone();
        self.engine
            .store()
            .with(move |s| s.transaction(|tx| tx.stop_instance(&id)))
            .await?;
        Ok(())
    }

    /// Claims runnable effects (up to the concurrency limit), runs them to completion, and
    /// reports what happened.
    pub async fn tick(&self) -> Result<TickReport, DispatchError> {
        let instance = self.instance.clone();
        let (ttl, limit) = (self.config.lease_ttl, self.config.max_concurrent);
        let claimed = self
            .engine
            .store()
            .with(move |s| {
                s.transaction(|tx| {
                    tx.heartbeat_instance(&instance)?;
                    tx.claim_effects(&ClaimParams {
                        instance_id: &instance,
                        limit,
                        lease_ttl: ttl,
                        control_kinds: Effect::CONTROL_KINDS,
                    })
                })
            })
            .await?;
        let mut report = TickReport {
            claimed: u32::try_from(claimed.len()).unwrap_or(u32::MAX),
            ..TickReport::default()
        };
        self.crash.point("tick:claimed")?;

        let mut set = JoinSet::new();
        for effect in claimed {
            let this = self.clone();
            set.spawn(async move { this.run_effect(effect).await });
        }
        while let Some(joined) = set.join_next().await {
            match joined {
                Ok(Ok(Finished::Completed)) => report.completed += 1,
                Ok(Ok(Finished::Requeued)) => report.requeued += 1,
                Ok(Ok(Finished::Failed)) => report.failed += 1,
                Ok(Ok(Finished::LeaseLost)) => report.lease_lost += 1,
                Ok(Err(e)) => {
                    set.abort_all();
                    return Err(e);
                }
                Err(join) if join.is_panic() => std::panic::resume_unwind(join.into_panic()),
                Err(_) => {}
            }
        }
        Ok(report)
    }

    async fn run_effect(&self, effect: EffectRow) -> Result<Finished, DispatchError> {
        // The executor runs in its own task so a panic in adapter code cannot take the
        // dispatcher down (NFR-008); it is reported as a retryable failure instead.
        let executor = Arc::clone(&self.executor);
        let for_task = effect.clone();
        let mut handle = tokio::spawn(async move { executor.execute(&for_task).await });

        let outcome = tokio::select! {
            joined = &mut handle => Some(match joined {
                Ok(r) => r,
                Err(e) if e.is_panic() => Err(ExecError::Retryable("executor panicked".into())),
                Err(e) => Err(ExecError::Retryable(format!("executor task failed: {e}"))),
            }),
            () = self.keep_lease(&effect) => {
                handle.abort();
                None
            }
        };
        let Some(outcome) = outcome else {
            return Ok(Finished::LeaseLost);
        };
        self.crash.point("effect:executed")?;

        let instance = self.instance.clone();
        let finished = match outcome {
            Ok(o) => {
                self.engine
                    .finish_effect(&effect, &instance, o.result, o.command)
                    .await?
            }
            Err(ExecError::Retryable(msg)) => {
                self.engine
                    .fail_effect(&effect, &instance, &msg, Some(backoff(effect.attempts)))
                    .await?
            }
            Err(ExecError::Fatal(msg)) => {
                self.engine
                    .fail_effect(&effect, &instance, &msg, None)
                    .await?
            }
        };
        Ok(finished)
    }

    /// Renews the lease until it is lost. Completes (returns) only when renewal fails, which
    /// the caller treats as "stop, someone else may own this effect now".
    async fn keep_lease(&self, effect: &EffectRow) {
        loop {
            tokio::time::sleep(self.config.heartbeat).await;
            let (id, instance, ttl) = (effect.id, self.instance.clone(), self.config.lease_ttl);
            let renewed = self
                .engine
                .store()
                .with(move |s| s.transaction(|tx| tx.renew_lease(id, &instance, ttl)))
                .await;
            if !matches!(renewed, Ok(true)) {
                return;
            }
        }
    }

    /// Runs ticks until `shutdown` turns true. Sleeps when idle, wakes on [`Dispatcher::wake`].
    pub async fn run(&self, mut shutdown: watch::Receiver<bool>) -> Result<(), DispatchError> {
        while !*shutdown.borrow() {
            let report = self.tick().await?;
            if report.claimed == 0 {
                tokio::select! {
                    () = self.wake.notified() => {}
                    () = tokio::time::sleep(self.config.poll_interval) => {}
                    _ = shutdown.changed() => {}
                }
            }
        }
        self.shutdown().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_doubles_and_caps() {
        let secs: Vec<u64> = (1..=8).map(|a| backoff(a).as_secs()).collect();
        assert_eq!(secs, [10, 20, 40, 80, 120, 120, 120, 120]);
        assert_eq!(backoff(0).as_secs(), 10);
    }
}
