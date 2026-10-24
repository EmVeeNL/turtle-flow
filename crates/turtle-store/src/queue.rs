//! The effect queue: claiming with leases, heartbeats, completion, failure with backoff, and
//! recovery after a restart (`docs/adr/ADR-011`, `ADR-017`, `docs/WORKFLOW.md` §10).
//!
//! Effects are *at-least-once*: a crash after a side effect but before completion makes the
//! effect run again, so executors must be idempotent (probe the real world first). Everything in
//! this module is about making that safe and bounded.

use std::time::Duration;

use rusqlite::{OptionalExtension, params, params_from_iter};
use serde_json::Value;
use time::OffsetDateTime;
use turtle_domain::{Actor, EffectId};

use crate::clock::format_ts;
use crate::error::{Result, StoreError};
use crate::store::{EffectRow, NewEvent, Tx};

#[derive(Debug, Clone, Copy)]
pub struct ClaimParams<'a> {
    pub instance_id: &'a str,
    pub limit: u32,
    pub lease_ttl: Duration,
    /// Effect kinds that ignore holds and per-job ordering (stop work, release, notify).
    pub control_kinds: &'a [&'a str],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectFate {
    Requeued,
    Failed,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct RecoveryReport {
    pub instances_stopped: u32,
    pub effects_requeued: u32,
    /// Effects that had already used all attempts; the caller must tell the workflow.
    pub effects_failed: Vec<EffectRow>,
}

const TERMINAL_OR_BLOCKED: &str = "('blocked','done','cancelled','failed')";
const SELECT_EFFECT: &str = "SELECT e.id, e.job_id, e.kind, e.idempotency_key, e.payload, e.status,
    e.attempts, e.max_attempts, e.created_event_seq FROM effects e";

fn row_to_effect(r: &rusqlite::Row<'_>) -> rusqlite::Result<EffectRow> {
    let id: String = r.get(0)?;
    let job: Option<String> = r.get(1)?;
    let payload: String = r.get(4)?;
    Ok(EffectRow {
        id: id.parse().unwrap_or_default(),
        job_id: job.and_then(|j| j.parse().ok()),
        kind: r.get(2)?,
        idempotency_key: r.get(3)?,
        payload: serde_json::from_str(&payload).unwrap_or(Value::Null),
        status: r.get(5)?,
        attempts: r.get(6)?,
        max_attempts: r.get(7)?,
        created_event_seq: r.get(8)?,
    })
}

fn placeholders(n: usize) -> String {
    vec!["?"; n].join(",")
}

impl Tx<'_> {
    fn ts_in(&self, d: Duration) -> Result<String> {
        let d = time::Duration::try_from(d)
            .map_err(|_| StoreError::Corrupt("duration out of range".into()))?;
        Ok(format_ts(self.clock.now() + d)?)
    }

    // ------------------------------------------------------------ instances

    /// Registers this daemon process. Returns its instance id (carried by every lease).
    pub fn register_instance(&mut self, version: &str, pid: u32) -> Result<String> {
        let id = ulid::Ulid::generate().to_string();
        let now = self.now()?;
        self.tx.execute(
            "INSERT INTO daemon_instances(instance_id, pid, started_at, heartbeat_at, version)
             VALUES (?1, ?2, ?3, ?3, ?4)",
            params![id, pid, now, version],
        )?;
        Ok(id)
    }

    pub fn heartbeat_instance(&mut self, instance_id: &str) -> Result<()> {
        let now = self.now()?;
        self.tx.execute(
            "UPDATE daemon_instances SET heartbeat_at = ?1 WHERE instance_id = ?2 AND stopped_at IS NULL",
            params![now, instance_id],
        )?;
        Ok(())
    }

    pub fn stop_instance(&mut self, instance_id: &str) -> Result<()> {
        let now = self.now()?;
        self.tx.execute(
            "UPDATE daemon_instances SET stopped_at = ?1 WHERE instance_id = ?2 AND stopped_at IS NULL",
            params![now, instance_id],
        )?;
        Ok(())
    }

    // ------------------------------------------------------------ claiming

    /// Claims up to `limit` runnable effects for `instance_id`.
    ///
    /// Rules, all enforced here so no caller can get them wrong:
    /// - pending effects of blocked or finished jobs are cancelled (except control effects);
    /// - jobs with holds (paused, waiting for budget, rate limited) get no new non-control work;
    /// - per job, effects run strictly in creation order and one at a time (invariant J-2);
    /// - effects waiting out a retry backoff (`not_before`) are skipped.
    pub fn claim_effects(&mut self, p: &ClaimParams<'_>) -> Result<Vec<EffectRow>> {
        let now = self.now()?;
        let n = p.control_kinds.len();
        let ph = placeholders(n);

        self.tx.execute(
            &format!(
                "UPDATE effects SET status = 'cancelled', completed_at = ?1,
                        result = '{{\"cancelled\":\"job is blocked or finished\"}}'
                 WHERE status = 'pending' AND kind NOT IN ({ph})
                   AND job_id IN (SELECT id FROM jobs WHERE state IN {TERMINAL_OR_BLOCKED})"
            ),
            params_from_iter(std::iter::once(now.as_str()).chain(p.control_kinds.iter().copied())),
        )?;

        let sql = format!(
            "{SELECT_EFFECT} LEFT JOIN jobs j ON j.id = e.job_id
             WHERE e.status = 'pending'
               AND (e.not_before IS NULL OR e.not_before <= ?1)
               AND (
                 e.kind IN ({ph})
                 OR (
                   (j.id IS NULL OR j.holds_json = '[]')
                   AND NOT EXISTS (
                     SELECT 1 FROM effects o
                     WHERE o.job_id = e.job_id AND o.id <> e.id
                       AND o.kind NOT IN ({ph})
                       AND (o.status = 'claimed'
                            OR (o.status = 'pending'
                                AND (o.created_event_seq, o.idempotency_key)
                                    < (e.created_event_seq, e.idempotency_key)))
                   )
                 )
               )
             ORDER BY e.created_event_seq, e.idempotency_key
             LIMIT ?{}",
            2 * n + 2
        );
        let mut args: Vec<&dyn rusqlite::ToSql> = Vec::new();
        args.push(&now);
        for k in p.control_kinds {
            args.push(k);
        }
        for k in p.control_kinds {
            args.push(k);
        }
        let limit = i64::from(p.limit);
        args.push(&limit);
        let candidates: Vec<EffectRow> = {
            let mut stmt = self.tx.prepare(&sql)?;
            let rows = stmt.query_map(args.as_slice(), row_to_effect)?;
            rows.collect::<Result<_, _>>()?
        };

        // Two candidates of the same job (ordering rule applies to *stored* state, not to the
        // candidate list): keep only the first per job so one claim batch cannot run a job's
        // effects concurrently.
        let mut seen_jobs = std::collections::HashSet::new();
        let mut claimed = Vec::new();
        let expires = self.ts_in(p.lease_ttl)?;
        for mut e in candidates {
            if let Some(job) = e.job_id
                && e.kind_is_ordered(p.control_kinds)
                && !seen_jobs.insert(job)
            {
                continue;
            }
            self.tx.execute(
                "UPDATE effects SET status = 'claimed', attempts = attempts + 1,
                        lease_instance_id = ?1, lease_expires_at = ?2
                 WHERE id = ?3 AND status = 'pending'",
                params![p.instance_id, expires, e.id.to_string()],
            )?;
            e.attempts += 1;
            e.status = "claimed".into();
            claimed.push(e);
        }
        Ok(claimed)
    }

    /// Extends the lease. Returns `false` if this instance no longer holds it.
    pub fn renew_lease(&mut self, id: EffectId, instance_id: &str, ttl: Duration) -> Result<bool> {
        let expires = self.ts_in(ttl)?;
        let n = self.tx.execute(
            "UPDATE effects SET lease_expires_at = ?1
             WHERE id = ?2 AND status = 'claimed' AND lease_instance_id = ?3",
            params![expires, id.to_string(), instance_id],
        )?;
        Ok(n == 1)
    }

    /// Marks a claimed effect completed. Fails with [`StoreError::LeaseLost`] if the lease is
    /// gone. Call it in the same transaction that applies the follow-up decision.
    pub fn complete_effect(
        &mut self,
        id: EffectId,
        instance_id: &str,
        result: &Value,
    ) -> Result<()> {
        let now = self.now()?;
        let n = self.tx.execute(
            "UPDATE effects SET status = 'completed', completed_at = ?1, result = ?2,
                    lease_instance_id = NULL, lease_expires_at = NULL
             WHERE id = ?3 AND status = 'claimed' AND lease_instance_id = ?4",
            params![now, result.to_string(), id.to_string(), instance_id],
        )?;
        if n == 1 {
            Ok(())
        } else {
            Err(StoreError::LeaseLost { effect: id })
        }
    }

    /// Records a failed attempt. With `retry_in` and attempts left the effect goes back to
    /// `pending` after the backoff; otherwise it is `failed` for good.
    pub fn fail_effect(
        &mut self,
        id: EffectId,
        instance_id: &str,
        error: &str,
        retry_in: Option<Duration>,
    ) -> Result<EffectFate> {
        let now = self.now()?;
        let (attempts, max, job): (u32, u32, Option<String>) = self
            .tx
            .query_row(
                "SELECT attempts, max_attempts, job_id FROM effects
                 WHERE id = ?1 AND status = 'claimed' AND lease_instance_id = ?2",
                params![id.to_string(), instance_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?
            .ok_or(StoreError::LeaseLost { effect: id })?;
        let result = serde_json::json!({ "error": error, "attempts": attempts }).to_string();
        let fate = match retry_in {
            Some(d) if attempts < max => {
                let not_before = self.ts_in(d)?;
                self.tx.execute(
                    "UPDATE effects SET status = 'pending', not_before = ?1, result = ?2,
                            lease_instance_id = NULL, lease_expires_at = NULL WHERE id = ?3",
                    params![not_before, result, id.to_string()],
                )?;
                EffectFate::Requeued
            }
            _ => {
                self.tx.execute(
                    "UPDATE effects SET status = 'failed', completed_at = ?1, result = ?2,
                            lease_instance_id = NULL, lease_expires_at = NULL WHERE id = ?3",
                    params![now, result, id.to_string()],
                )?;
                EffectFate::Failed
            }
        };
        self.append_effect_event(job.as_deref(), fate, id, error)?;
        Ok(fate)
    }

    fn append_effect_event(
        &mut self,
        job: Option<&str>,
        fate: EffectFate,
        id: EffectId,
        error: &str,
    ) -> Result<()> {
        let kind = match fate {
            EffectFate::Requeued => "effect.requeued",
            EffectFate::Failed => "effect.failed",
        };
        let job_id = job.and_then(|j| j.parse().ok());
        let project_id = match job_id {
            Some(j) => Some(self.load_job(j)?.project_id),
            None => None,
        };
        self.append_event(&NewEvent {
            project_id,
            job_id,
            kind: kind.into(),
            payload: serde_json::json!({ "effect": id.to_string(), "error": error }),
            actor: Actor::Scheduler,
        })?;
        Ok(())
    }

    // ------------------------------------------------------------ recovery

    /// Startup recovery (`docs/WORKFLOW.md` §10): any instance that did not stop cleanly is
    /// dead (the daemon lock guarantees a single live instance), so its claimed effects go back
    /// to `pending`, or to `failed` if they already used every attempt.
    pub fn recover_after_restart(&mut self, current_instance: &str) -> Result<RecoveryReport> {
        let now = self.now()?;
        let instances_stopped = u32::try_from(self.tx.execute(
            "UPDATE daemon_instances SET stopped_at = ?1
             WHERE stopped_at IS NULL AND instance_id <> ?2",
            params![now, current_instance],
        )?)
        .unwrap_or(u32::MAX);
        let mut report = RecoveryReport {
            instances_stopped,
            ..RecoveryReport::default()
        };

        let orphaned: Vec<EffectRow> = {
            let mut stmt = self.tx.prepare(&format!(
                "{SELECT_EFFECT} WHERE e.status = 'claimed'
                   AND (e.lease_instance_id IS NULL OR e.lease_instance_id <> ?1)
                 ORDER BY e.created_event_seq, e.idempotency_key"
            ))?;
            let rows = stmt.query_map([current_instance], row_to_effect)?;
            rows.collect::<Result<_, _>>()?
        };
        for e in orphaned {
            let job = e.job_id.map(|j| j.to_string());
            if e.attempts >= e.max_attempts {
                self.tx.execute(
                    "UPDATE effects SET status = 'failed', completed_at = ?1,
                            result = '{\"error\":\"abandoned: all attempts were used before a restart\"}',
                            lease_instance_id = NULL, lease_expires_at = NULL WHERE id = ?2",
                    params![now, e.id.to_string()],
                )?;
                self.append_effect_event(
                    job.as_deref(),
                    EffectFate::Failed,
                    e.id,
                    "abandoned across restart",
                )?;
                report.effects_failed.push(EffectRow {
                    status: "failed".into(),
                    ..e
                });
            } else {
                self.tx.execute(
                    "UPDATE effects SET status = 'pending', lease_instance_id = NULL,
                            lease_expires_at = NULL WHERE id = ?1",
                    params![e.id.to_string()],
                )?;
                self.append_effect_event(
                    job.as_deref(),
                    EffectFate::Requeued,
                    e.id,
                    "lease owner restarted",
                )?;
                report.effects_requeued += 1;
            }
        }
        self.append_event(&NewEvent {
            project_id: None,
            job_id: None,
            kind: "daemon.recovered".into(),
            payload: serde_json::json!({
                "instance": current_instance,
                "instances_stopped": report.instances_stopped,
                "effects_requeued": report.effects_requeued,
                "effects_failed": report.effects_failed.len(),
            }),
            actor: Actor::System,
        })?;
        Ok(report)
    }

    /// Effects not yet finished (pending or claimed) for tests and diagnostics.
    pub fn unfinished_effect_count(&self) -> Result<u64> {
        let n: i64 = self.tx.query_row(
            "SELECT count(*) FROM effects WHERE status IN ('pending','claimed')",
            [],
            |r| r.get(0),
        )?;
        Ok(u64::try_from(n).unwrap_or(0))
    }

    #[doc(hidden)]
    pub fn now_for_tests(&self) -> OffsetDateTime {
        self.clock.now()
    }
}

impl EffectRow {
    /// Control effects are exempt from per-job ordering.
    fn kind_is_ordered(&self, control_kinds: &[&str]) -> bool {
        !control_kinds.contains(&self.kind.as_str())
    }
}
