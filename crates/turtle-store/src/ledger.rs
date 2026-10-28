//! Budgets, reservations and the token ledger (`docs/TOKEN-INTELLIGENCE.md` §4,
//! `docs/DATA-MODEL.md` §3.4), plus the agent-run and usage records they hang off.
//!
//! Unit: *tokens processed*, which counts cache reads and writes (`TokenUsage::total`). That is
//! a deliberate, simple choice for this first ledger; currency budgets come with the price
//! table.
//!
//! Invariant B-1: when a reservation is made, `settled + reserved + requested ≤ limit` for every
//! hard budget that applies, checked and recorded in one transaction across all scopes. A
//! settlement may legitimately overrun (providers report usage after the fact); it is booked
//! truthfully and raises a `ledger.overrun` event.
//!
//! Every ledger entry points at the event that caused it, and `Store::verify_ledger` recomputes
//! each budget's cached `settled`/`reserved` columns from the entries, so the projection can be
//! rebuilt and checked.

use rusqlite::{OptionalExtension, params};
use serde_json::json;
use time::{OffsetDateTime, Time};
use turtle_domain::{Actor, BudgetId, JobId, ProjectId, ReservationId, Role, RunId, UsageValue};
use turtle_tokens::{EstimateBasis, ReservationSize};

use crate::clock::format_ts;
use crate::error::{Result, StoreError};
use crate::store::{NewEvent, Store, Tx, db_int};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetScope {
    /// All of Turtle Flow, per calendar month.
    Global,
    /// One project, per calendar month.
    Project(ProjectId),
    /// One job, for its lifetime.
    Job(JobId),
}

impl BudgetScope {
    fn kind(self) -> &'static str {
        match self {
            Self::Global => "Turtle Flow",
            Self::Project(_) => "project",
            Self::Job(_) => "job",
        }
    }

    /// Name for messages: `job`, `project` or `global`.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::Project(_) => "project",
            Self::Job(_) => "job",
        }
    }

    fn id(self) -> Option<String> {
        match self {
            Self::Global => None,
            Self::Project(p) => Some(p.to_string()),
            Self::Job(j) => Some(j.to_string()),
        }
    }

    const fn monthly(self) -> bool {
        !matches!(self, Self::Job(_))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BudgetRecord {
    pub id: BudgetId,
    pub scope: BudgetScope,
    pub period_start: Option<String>,
    pub limit_tokens: u64,
    pub hard: bool,
    pub settled_tokens: u64,
    pub reserved_tokens: u64,
}

impl BudgetRecord {
    /// Tokens that can still be reserved (zero when over the limit).
    #[must_use]
    pub fn headroom(&self) -> u64 {
        self.limit_tokens
            .saturating_sub(self.settled_tokens + self.reserved_tokens)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReservationOutcome {
    /// No budget applies to this job; the run is not metered.
    Unmetered,
    Reserved {
        reservation: ReservationId,
        /// Soft budgets that this reservation takes over their limit (a warning, not a stop).
        soft_exceeded: Vec<BudgetId>,
    },
    /// A hard budget cannot cover the request. Nothing was reserved anywhere.
    Denied {
        budget: BudgetRecord,
        requested: u64,
    },
}

#[derive(Debug, Clone)]
pub struct NewRun {
    pub id: RunId,
    pub job_id: JobId,
    pub role: Role,
    pub stage: String,
    pub provider: String,
    pub model: String,
    pub adapter_version: String,
    pub context_manifest_rev: Option<String>,
}

#[derive(Debug)]
pub struct NewRoutingDecision<'a> {
    pub run: RunId,
    pub job: JobId,
    pub role: Role,
    pub chosen: &'a str,
    pub candidates: &'a serde_json::Value,
    pub reason: &'a str,
    pub override_used: bool,
}

#[derive(Debug, Clone)]
pub struct RunRecord {
    pub id: String,
    pub job_id: String,
    pub cli_version: Option<String>,
    pub routing_reason: Option<String>,
    /// Content hash of the capability snapshot the run started with.
    pub capability_snapshot_hash: Option<String>,
    pub role: String,
    pub provider: String,
    pub model: String,
    pub status: String,
    pub input: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub output: u64,
    pub provenance: String,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
    pub duration_ms: Option<u64>,
    pub transcript_blob: Option<turtle_domain::Sha256>,
}

#[derive(Debug, Clone)]
pub struct OpenRun {
    pub id: RunId,
    pub job_id: JobId,
    pub provider: String,
    pub role: Role,
    pub pid: Option<u32>,
    pub spool_path: Option<String>,
}

/// How a run ended, for the `agent_runs` row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunEnd {
    Succeeded,
    Failed { retryable: bool },
    Cancelled,
    Abandoned,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobUsage {
    pub runs: u64,
    pub tokens_reported: u64,
    pub tokens_estimated: u64,
    /// Runs whose usage is unknown; they are not counted in either total.
    pub runs_unavailable: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LedgerVerdict {
    Consistent { budgets: u64, entries: u64 },
    Mismatch { budget: BudgetId, detail: String },
}

fn month_start(now: OffsetDateTime) -> Result<String> {
    let start = now
        .replace_day(1)
        .map_err(|e| StoreError::Corrupt(e.to_string()))?
        .replace_time(Time::MIDNIGHT);
    Ok(format_ts(start)?)
}

fn to_u64(v: i64) -> u64 {
    u64::try_from(v).unwrap_or(0)
}

const SELECT_BUDGET: &str = "SELECT id, scope_kind, scope_id, period_start, limit_tokens, hard,
    settled_tokens, reserved_tokens FROM budgets";

fn read_budget(r: &rusqlite::Row<'_>) -> rusqlite::Result<BudgetRecord> {
    let kind: String = r.get(1)?;
    let scope_id: Option<String> = r.get(2)?;
    let bad = |what: &str| {
        rusqlite::Error::FromSqlConversionFailure(
            2,
            rusqlite::types::Type::Text,
            format!("bad {what} id").into(),
        )
    };
    let scope = match (kind.as_str(), scope_id) {
        ("project", Some(id)) => BudgetScope::Project(id.parse().map_err(|_| bad("project"))?),
        ("job", Some(id)) => BudgetScope::Job(id.parse().map_err(|_| bad("job"))?),
        _ => BudgetScope::Global,
    };
    let id: String = r.get(0)?;
    Ok(BudgetRecord {
        id: id.parse().map_err(|_| bad("budget"))?,
        scope,
        period_start: r.get(3)?,
        limit_tokens: to_u64(r.get::<_, Option<i64>>(4)?.unwrap_or(0)),
        hard: r.get::<_, i64>(5)? == 1,
        settled_tokens: to_u64(r.get(6)?),
        reserved_tokens: to_u64(r.get(7)?),
    })
}

impl Tx<'_> {
    /// Links a captured transcript blob to its run, registering blob metadata atomically.
    pub fn set_transcript_blob(
        &mut self,
        run: RunId,
        hash: turtle_domain::Sha256,
        size: u64,
    ) -> Result<()> {
        let now = self.now()?;
        let size =
            i64::try_from(size).map_err(|_| StoreError::Corrupt("transcript too large".into()))?;
        self.tx.execute(
            "INSERT INTO blobs(sha256, size_bytes, media_type, ref_count, created_at) VALUES (?1, ?2, 'application/x-ndjson', 1, ?3) ON CONFLICT(sha256) DO UPDATE SET ref_count = ref_count + 1",
            params![hash.to_hex(), size, now],
        )?;
        self.tx.execute(
            "UPDATE agent_runs SET transcript_blob = ?1 WHERE id = ?2",
            params![hash.to_hex(), run.to_string()],
        )?;
        Ok(())
    }

    fn latest_budget(&self, scope: BudgetScope) -> Result<Option<BudgetRecord>> {
        Ok(self
            .tx
            .query_row(
                &format!(
                    "{SELECT_BUDGET} WHERE scope_kind = ?1 AND IFNULL(scope_id,'') = ?2
                     ORDER BY IFNULL(period_start,'') DESC LIMIT 1"
                ),
                params![scope.kind(), scope.id().unwrap_or_default()],
                read_budget,
            )
            .optional()?)
    }

    /// The budget row for the current period, rolling a monthly budget over into a new period
    /// (same limit, zero spend) when the month has changed.
    fn current_budget(&mut self, scope: BudgetScope) -> Result<Option<BudgetRecord>> {
        let Some(latest) = self.latest_budget(scope)? else {
            return Ok(None);
        };
        if !scope.monthly() {
            return Ok(Some(latest));
        }
        let start = month_start(self.clock.now())?;
        if latest.period_start.as_deref() == Some(start.as_str()) {
            return Ok(Some(latest));
        }
        let id = BudgetId::new();
        self.tx.execute(
            "INSERT INTO budgets(id, scope_kind, scope_id, period, period_start, limit_tokens, hard)
             VALUES (?1, ?2, ?3, 'monthly', ?4, ?5, ?6)",
            params![
                id.to_string(),
                scope.kind(),
                scope.id(),
                start,
                db_int(latest.limit_tokens)?,
                i64::from(latest.hard)
            ],
        )?;
        self.latest_budget(scope)
    }

    /// Sets (or changes) the token limit of a budget. A change is a `limit_change` ledger entry
    /// tied to an event, so it shows in the audit log.
    pub fn set_budget(
        &mut self,
        scope: BudgetScope,
        limit_tokens: u64,
        hard: bool,
        job: Option<JobId>,
        actor: &Actor,
    ) -> Result<BudgetRecord> {
        let existing = self.current_budget(scope)?;
        let (id, previous) = if let Some(b) = existing {
            self.tx.execute(
                "UPDATE budgets SET limit_tokens = ?1, hard = ?2, version = version + 1 WHERE id = ?3",
                params![db_int(limit_tokens)?, i64::from(hard), b.id.to_string()],
            )?;
            (b.id, Some(b.limit_tokens))
        } else {
            let id = BudgetId::new();
            let (period, start) = if scope.monthly() {
                ("monthly", Some(month_start(self.clock.now())?))
            } else {
                ("lifetime", None)
            };
            self.tx.execute(
                "INSERT INTO budgets(id, scope_kind, scope_id, period, period_start, limit_tokens, hard)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    id.to_string(),
                    scope.kind(),
                    scope.id(),
                    period,
                    start,
                    db_int(limit_tokens)?,
                    i64::from(hard)
                ],
            )?;
            (id, None)
        };
        let seq = self.append_event(&NewEvent {
            project_id: None,
            job_id: job,
            kind: "ledger.limit_changed".into(),
            payload: json!({
                "budget": id.to_string(), "scope": scope.kind(), "scope_id": scope.id(),
                "from": previous, "to": limit_tokens, "hard": hard,
            }),
            actor: actor.clone(),
        })?;
        self.ledger_entry(
            &id,
            None,
            "limit_change",
            (0, 0),
            Some("limit changed"),
            seq,
        )?;
        self.budget(id)
    }

    pub fn budget(&self, id: BudgetId) -> Result<BudgetRecord> {
        self.tx
            .query_row(
                &format!("{SELECT_BUDGET} WHERE id = ?1"),
                [id.to_string()],
                read_budget,
            )
            .optional()?
            .ok_or_else(|| StoreError::NotFound(format!("budget {id}")))
    }

    /// The budgets that apply to a job's runs: its own, its project's and the global one.
    pub fn budgets_for_job(&mut self, job: JobId) -> Result<Vec<BudgetRecord>> {
        let project = self.load_job(job)?.project_id;
        let mut out = Vec::new();
        for scope in [
            BudgetScope::Job(job),
            BudgetScope::Project(project),
            BudgetScope::Global,
        ] {
            out.extend(self.current_budget(scope)?);
        }
        Ok(out)
    }

    fn ledger_entry(
        &mut self,
        budget: &BudgetId,
        reservation: Option<ReservationId>,
        kind: &str,
        (reserved_delta, settled_delta): (i64, i64),
        reason: Option<&str>,
        event_seq: i64,
    ) -> Result<()> {
        let now = self.now()?;
        self.tx.execute(
            "INSERT INTO ledger_entries(id, budget_id, reservation_id, kind, tokens_delta_reserved,
                                        tokens_delta_settled, reason, event_seq, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                ulid::Ulid::generate().to_string(),
                budget.to_string(),
                reservation.map(|r| r.to_string()),
                kind,
                reserved_delta,
                settled_delta,
                reason,
                event_seq,
                now
            ],
        )?;
        if reserved_delta != 0 || settled_delta != 0 {
            self.tx.execute(
                "UPDATE budgets SET reserved_tokens = reserved_tokens + ?1,
                        settled_tokens = settled_tokens + ?2, version = version + 1 WHERE id = ?3",
                params![reserved_delta, settled_delta, budget.to_string()],
            )?;
        }
        Ok(())
    }

    /// Reserves `tokens` for a run against every budget that applies, all or nothing (B-1).
    /// The run row must exist (`begin_run`).
    pub fn reserve(&mut self, run: RunId, job: JobId, tokens: u64) -> Result<ReservationOutcome> {
        self.reserve_inner(run, job, tokens, None)
    }

    pub fn reserve_estimated(
        &mut self,
        run: RunId,
        job: JobId,
        size: &ReservationSize,
    ) -> Result<ReservationOutcome> {
        let basis = match size.estimate.basis {
            EstimateBasis::ColdStart => json!({"kind": "cold_start"}),
            EstimateBasis::Bucket {
                level,
                observations,
            } => json!({"kind": "bucket", "level": level, "observations": observations}),
        };
        self.reserve_inner(
            run,
            job,
            size.tokens,
            Some(json!({
                "p10": size.estimate.p10, "p50": size.estimate.p50, "p90": size.estimate.p90,
                "confidence": size.estimate.confidence.label(), "basis": basis,
            })),
        )
    }

    fn reserve_inner(
        &mut self,
        run: RunId,
        job: JobId,
        tokens: u64,
        estimate: Option<serde_json::Value>,
    ) -> Result<ReservationOutcome> {
        let budgets = self.budgets_for_job(job)?;
        if budgets.is_empty() {
            return Ok(ReservationOutcome::Unmetered);
        }
        let mut soft_exceeded = Vec::new();
        for b in &budgets {
            if tokens > b.headroom() {
                if b.hard {
                    self.append_event(&NewEvent {
                        project_id: None,
                        job_id: Some(job),
                        kind: "ledger.denied".into(),
                        payload: json!({
                            "run": run.to_string(), "budget": b.id.to_string(),
                            "scope": b.scope.kind(), "requested": tokens, "headroom": b.headroom(),
                        }),
                        actor: Actor::Scheduler,
                    })?;
                    return Ok(ReservationOutcome::Denied {
                        budget: b.clone(),
                        requested: tokens,
                    });
                }
                soft_exceeded.push(b.id);
            }
        }
        let reservation = ReservationId::new();
        let now = self.now()?;
        self.tx.execute(
            "INSERT INTO reservations(id, run_id, tokens, status, created_at)
             VALUES (?1, ?2, ?3, 'open', ?4)",
            params![
                reservation.to_string(),
                run.to_string(),
                db_int(tokens)?,
                now
            ],
        )?;
        self.tx.execute(
            "UPDATE agent_runs SET reservation_id = ?1 WHERE id = ?2",
            params![reservation.to_string(), run.to_string()],
        )?;
        let seq = self.append_event(&NewEvent {
            project_id: None,
            job_id: Some(job),
            kind: "ledger.reserved".into(),
            payload: json!({
                "run": run.to_string(), "tokens": tokens,
                "budgets": budgets.iter().map(|b| b.id.to_string()).collect::<Vec<_>>(),
                "soft_exceeded": soft_exceeded.iter().map(ToString::to_string).collect::<Vec<_>>(),
                "estimate": estimate,
            }),
            actor: Actor::Scheduler,
        })?;
        let delta = db_int(tokens)?;
        for b in &budgets {
            self.ledger_entry(&b.id, Some(reservation), "reserve", (delta, 0), None, seq)?;
        }
        Ok(ReservationOutcome::Reserved {
            reservation,
            soft_exceeded,
        })
    }

    /// The open reservation of a run, if it has one.
    fn open_reservation(&self, run: RunId) -> Result<Option<(ReservationId, u64, JobId)>> {
        let row: Option<(String, i64, String)> = self
            .tx
            .query_row(
                "SELECT r.id, r.tokens, a.job_id FROM reservations r
                 JOIN agent_runs a ON a.id = r.run_id
                 WHERE r.run_id = ?1 AND r.status = 'open'",
                [run.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        row.map(|(id, tokens, job)| {
            let bad = |_| StoreError::Corrupt("bad id in reservation".into());
            Ok((
                id.parse().map_err(bad)?,
                to_u64(tokens),
                job.parse().map_err(bad)?,
            ))
        })
        .transpose()
    }

    fn reserved_budgets(&self, reservation: ReservationId) -> Result<Vec<BudgetId>> {
        let mut stmt = self.tx.prepare(
            "SELECT budget_id FROM ledger_entries WHERE reservation_id = ?1 AND kind = 'reserve'
             ORDER BY created_at, id",
        )?;
        let rows = stmt
            .query_map([reservation.to_string()], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|s| {
                s.parse()
                    .map_err(|_| StoreError::Corrupt("bad budget id".into()))
            })
            .collect()
    }

    /// Closes a run's reservation with what was actually used. Idempotent: returns `false` when
    /// there is nothing open (already settled, released, or never metered).
    pub fn settle(&mut self, run: RunId, actual_tokens: u64) -> Result<bool> {
        let Some((reservation, reserved, job)) = self.open_reservation(run)? else {
            return Ok(false);
        };
        let budgets = self.reserved_budgets(reservation)?;
        let seq = self.append_event(&NewEvent {
            project_id: None,
            job_id: Some(job),
            kind: "ledger.settled".into(),
            payload: json!({ "run": run.to_string(), "reserved": reserved, "actual": actual_tokens }),
            actor: Actor::Scheduler,
        })?;
        let (released, settled) = (-db_int(reserved)?, db_int(actual_tokens)?);
        for b in &budgets {
            self.ledger_entry(
                b,
                Some(reservation),
                "settle",
                (released, settled),
                None,
                seq,
            )?;
        }
        self.close_reservation(reservation, "settled")?;
        for b in budgets {
            let record = self.budget(b)?;
            if record.hard && record.settled_tokens + record.reserved_tokens > record.limit_tokens {
                self.append_event(&NewEvent {
                    project_id: None,
                    job_id: Some(job),
                    kind: "ledger.overrun".into(),
                    payload: json!({
                        "budget": record.id.to_string(), "limit": record.limit_tokens,
                        "settled": record.settled_tokens, "reserved": record.reserved_tokens,
                    }),
                    actor: Actor::Scheduler,
                })?;
            }
        }
        Ok(true)
    }

    /// Gives a run's reservation back without charging anything. Idempotent.
    pub fn release(&mut self, run: RunId, reason: &str) -> Result<bool> {
        let Some((reservation, reserved, job)) = self.open_reservation(run)? else {
            return Ok(false);
        };
        let budgets = self.reserved_budgets(reservation)?;
        let seq = self.append_event(&NewEvent {
            project_id: None,
            job_id: Some(job),
            kind: "ledger.released".into(),
            payload: json!({ "run": run.to_string(), "reserved": reserved, "reason": reason }),
            actor: Actor::Scheduler,
        })?;
        let released = -db_int(reserved)?;
        for b in &budgets {
            self.ledger_entry(
                b,
                Some(reservation),
                "release",
                (released, 0),
                Some(reason),
                seq,
            )?;
        }
        self.close_reservation(reservation, "released")?;
        Ok(true)
    }

    fn close_reservation(&mut self, id: ReservationId, status: &str) -> Result<()> {
        let now = self.now()?;
        self.tx.execute(
            "UPDATE reservations SET status = ?1, closed_at = ?2 WHERE id = ?3",
            params![status, now, id.to_string()],
        )?;
        Ok(())
    }

    // ------------------------------------------------------------------ runs

    /// The run records its provider's latest capability snapshot (and that snapshot's CLI
    /// version) as the capability set it starts with; both stay NULL when none is recorded.
    ///
    /// An implementer or fixer run begun while one of its job's tasks is active (`running`,
    /// `gating` or `fixing`) belongs to that task. A job runs one task at a time, so there is at
    /// most one; stage-level runs keep `task_id = NULL`.
    pub fn begin_run(&mut self, run: &NewRun) -> Result<()> {
        let now = self.now()?;
        let attempt: i64 = self.tx.query_row(
            "SELECT COALESCE(MAX(attempt),0)+1 FROM agent_runs WHERE job_id=?1 AND stage=?2 AND role=?3",
            params![run.job_id.to_string(), run.stage, crate::store::to_s(&run.role)?],
            |r| r.get(0),
        )?;
        self.tx.execute(
            "INSERT INTO agent_runs(id, job_id, role, stage, provider, model, adapter_version,
                                    context_manifest_rev, attempt, status, started_at, created_at,
                                    cli_version, task_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?10, 'running', ?9, ?9,
                     (SELECT cli_version FROM capability_snapshots WHERE provider = ?5
                      ORDER BY version DESC LIMIT 1),
                     (SELECT id FROM tasks WHERE job_id = ?2 AND ?3 IN ('implementer', 'fixer')
                        AND state IN ('running', 'gating', 'fixing')
                      ORDER BY updated_at DESC, id DESC LIMIT 1))",
            params![
                run.id.to_string(),
                run.job_id.to_string(),
                crate::store::to_s(&run.role)?,
                run.stage,
                run.provider,
                run.model,
                run.adapter_version,
                run.context_manifest_rev,
                now,
                attempt,
            ],
        )?;
        self.tx.execute(
            "INSERT INTO run_capability_snapshots(run_id, snapshot_id)
             SELECT ?1, id FROM capability_snapshots WHERE provider = ?2
             ORDER BY version DESC LIMIT 1",
            params![run.id.to_string(), run.provider],
        )?;
        Ok(())
    }

    pub fn record_run_process(&mut self, run: RunId, pid: u32, spool_path: &str) -> Result<()> {
        let updated = self.tx.execute(
            "UPDATE agent_runs SET pid=?1, transcript_spool_path=?2 WHERE id=?3 AND status='running'",
            params![i64::from(pid), spool_path, run.to_string()],
        )?;
        if updated != 1 {
            return Err(StoreError::NotFound(format!("active run {run}")));
        }
        Ok(())
    }

    pub fn request_cancel_for_job(&mut self, job: JobId) -> Result<()> {
        self.tx.execute(
            "UPDATE agent_runs SET cancel_requested=1 WHERE job_id=?1 AND status IN ('queued','starting','running')",
            [job.to_string()],
        )?;
        Ok(())
    }

    /// The decision and its run link are written in the same transaction as the reservation.
    pub fn record_routing_decision(&mut self, decision: &NewRoutingDecision<'_>) -> Result<()> {
        let now = self.now()?;
        self.tx.execute(
            "INSERT INTO routing_decisions(id,job_id,role,chosen,candidates,reason,override,created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            params![decision.run.to_string(), decision.job.to_string(), crate::store::to_s(&decision.role)?, decision.chosen,
                decision.candidates.to_string(), decision.reason, i64::from(decision.override_used), now],
        )?;
        self.tx.execute(
            "UPDATE agent_runs SET routing_decision_id=?1 WHERE id=?1",
            [decision.run.to_string()],
        )?;
        Ok(())
    }

    pub fn latest_implementer_provider(&self, job: JobId) -> Result<Option<String>> {
        self.tx.query_row(
            "SELECT provider FROM agent_runs WHERE job_id=?1 AND role='implementer' AND status='succeeded'
             ORDER BY created_at DESC, id DESC LIMIT 1",
            [job.to_string()], |r| r.get(0),
        ).optional().map_err(Into::into)
    }

    /// Marks a run that never started (for example, refused for budget). It has no usage record:
    /// nothing was spent and nothing is unknown.
    pub fn cancel_run(&mut self, run: RunId, reason: &str) -> Result<()> {
        let now = self.now()?;
        self.tx.execute(
            "UPDATE agent_runs SET status = 'cancelled', error_message = ?1, ended_at = ?2
             WHERE id = ?3",
            params![reason, now, run.to_string()],
        )?;
        Ok(())
    }

    /// Ends a run: updates its row and records its usage with provenance. Unknown usage is
    /// stored as `unavailable` with zero counts that readers must not present as numbers.
    pub fn finish_run(
        &mut self,
        run: RunId,
        end: RunEnd,
        error: Option<&str>,
        usage: UsageValue,
    ) -> Result<()> {
        let now = self.now()?;
        let (status, retryable) = match end {
            RunEnd::Succeeded => ("succeeded", None),
            RunEnd::Failed { retryable } => ("failed", Some(i64::from(retryable))),
            RunEnd::Cancelled => ("cancelled", None),
            RunEnd::Abandoned => ("abandoned", None),
        };
        let (job, provider, model, project): (String, String, String, String) = self.tx.query_row(
            "SELECT a.job_id, a.provider, a.model, j.project_id FROM agent_runs a
             JOIN jobs j ON j.id = a.job_id WHERE a.id = ?1",
            [run.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )?;
        self.tx.execute(
            "UPDATE agent_runs SET status = ?1, error_retryable = ?2, error_message = ?3,
                    ended_at = ?4 WHERE id = ?5",
            params![status, retryable, error, now, run.to_string()],
        )?;
        let (provenance, u) = match usage {
            UsageValue::Reported { usage } => ("reported", Some(usage)),
            UsageValue::Estimated { usage } => ("estimated", Some(usage)),
            UsageValue::Unavailable => ("unavailable", None),
        };
        let u = u.unwrap_or_default();
        self.tx.execute(
            "INSERT INTO usage_records(id, run_id, project_id, job_id, provider, model,
                    input_tokens, cached_read, cached_write, output_tokens, reasoning_tokens,
                    provenance, billing_mode, recorded_at, task_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, 'unknown', ?13,
                     (SELECT task_id FROM agent_runs WHERE id = ?2))",
            params![
                ulid::Ulid::generate().to_string(),
                run.to_string(),
                project,
                job,
                provider,
                model,
                db_int(u.input)?,
                u.cached_input_read.map(db_int).transpose()?,
                u.cached_input_write.map(db_int).transpose()?,
                db_int(u.output)?,
                u.reasoning.map(db_int).transpose()?,
                provenance,
                now
            ],
        )?;
        Ok(())
    }

    /// Marks runs that were never finished (the daemon died mid-run) as abandoned and gives
    /// their reservations back. Their usage is unknown, so nothing is charged.
    pub fn abandon_open_runs(&mut self) -> Result<u32> {
        let mut stmt = self
            .tx
            .prepare("SELECT id FROM agent_runs WHERE status IN ('queued','starting','running')")?;
        let ids = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        drop(stmt);
        let now = self.now()?;
        for id in &ids {
            let run: RunId = id
                .parse()
                .map_err(|_| StoreError::Corrupt(format!("bad run id {id}")))?;
            self.release(run, "run abandoned by a restart")?;
            self.tx.execute(
                "UPDATE agent_runs SET status = 'abandoned', ended_at = ?1 WHERE id = ?2",
                params![now, id],
            )?;
        }
        Ok(u32::try_from(ids.len()).unwrap_or(u32::MAX))
    }
}

impl Store {
    pub fn run_cancel_requested(&self, run: RunId) -> Result<bool> {
        let requested: i64 = self.raw_connection().query_row(
            "SELECT cancel_requested FROM agent_runs WHERE id=?1",
            [run.to_string()],
            |r| r.get(0),
        )?;
        Ok(requested != 0)
    }

    pub fn live_cancel_requested(&self, job: JobId) -> Result<bool> {
        let count: i64 = self.raw_connection().query_row(
            "SELECT count(*) FROM agent_runs WHERE job_id=?1 AND cancel_requested=1 AND status IN ('queued','starting','running')",
            [job.to_string()], |r| r.get(0),
        )?;
        Ok(count > 0)
    }
    pub fn open_runs_for_recovery(&self) -> Result<Vec<OpenRun>> {
        let mut stmt = self.raw_connection().prepare(
            "SELECT id,job_id,provider,role,pid,transcript_spool_path FROM agent_runs
             WHERE status IN ('queued','starting','running')",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<i64>>(4)?,
                r.get::<_, Option<String>>(5)?,
            ))
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
            .into_iter()
            .map(|(id, job_id, provider, role, pid, spool_path)| {
                Ok(OpenRun {
                    id: id
                        .parse()
                        .map_err(|_| StoreError::Corrupt(format!("bad run id {id}")))?,
                    job_id: job_id
                        .parse()
                        .map_err(|_| StoreError::Corrupt(format!("bad job id {job_id}")))?,
                    provider,
                    role: serde_json::from_value(serde_json::Value::String(role))?,
                    pid: pid.and_then(|p| u32::try_from(p).ok()),
                    spool_path,
                })
            })
            .collect()
    }
    pub fn recent_runs(&self, limit: usize) -> Result<Vec<RunRecord>> {
        let mut runs = read_runs(self.raw_connection(), None, None)?;
        runs.reverse();
        runs.truncate(limit);
        Ok(runs)
    }
    pub fn runs_for_job(&self, job: JobId) -> Result<Vec<RunRecord>> {
        read_runs(self.raw_connection(), Some(job), None)
    }
    pub fn run_by_id(&self, id: &str) -> Result<RunRecord> {
        read_runs(self.raw_connection(), None, Some(id))?
            .into_iter()
            .next()
            .ok_or_else(|| StoreError::NotFound(format!("run {id}")))
    }
    /// Context manifest revision links recorded for this job's agent runs.
    pub fn run_context_manifest_revisions(&self, job: JobId) -> Result<Vec<Option<String>>> {
        let mut stmt = self.raw_connection().prepare(
            "SELECT context_manifest_rev FROM agent_runs WHERE job_id = ?1 ORDER BY created_at, id",
        )?;
        let rows = stmt.query_map([job.to_string()], |row| row.get(0))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    /// Every budget, newest period first within a scope.
    pub fn list_budgets(&self) -> Result<Vec<BudgetRecord>> {
        let conn = self.raw_connection();
        let mut stmt = conn.prepare(&format!(
            "{SELECT_BUDGET} ORDER BY scope_kind, IFNULL(scope_id,''), IFNULL(period_start,'') DESC"
        ))?;
        let rows = stmt
            .query_map([], read_budget)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// What a job's runs have used, split by how well the numbers are known.
    pub fn job_usage(&self, job: JobId) -> Result<JobUsage> {
        let conn = self.raw_connection();
        let mut usage = JobUsage {
            runs: 0,
            tokens_reported: 0,
            tokens_estimated: 0,
            runs_unavailable: 0,
        };
        let mut stmt = conn.prepare(
            "SELECT provenance, count(*),
                    sum(input_tokens + IFNULL(cached_read,0) + IFNULL(cached_write,0) + output_tokens)
             FROM usage_records WHERE job_id = ?1 GROUP BY provenance",
        )?;
        let rows = stmt
            .query_map([job.to_string()], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        for (provenance, runs, tokens) in rows {
            usage.runs += to_u64(runs);
            match provenance.as_str() {
                "reported" => usage.tokens_reported = to_u64(tokens),
                "estimated" => usage.tokens_estimated = to_u64(tokens),
                _ => usage.runs_unavailable = to_u64(runs),
            }
        }
        Ok(usage)
    }

    /// Recomputes every budget's cached `reserved`/`settled` from the ledger entries.
    pub fn verify_ledger(&self) -> Result<LedgerVerdict> {
        let conn = self.raw_connection();
        let mut stmt = conn.prepare(
            "SELECT b.id, b.reserved_tokens, b.settled_tokens,
                    IFNULL(sum(l.tokens_delta_reserved), 0), IFNULL(sum(l.tokens_delta_settled), 0),
                    count(l.id)
             FROM budgets b LEFT JOIN ledger_entries l ON l.budget_id = b.id GROUP BY b.id",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, i64>(4)?,
                    r.get::<_, i64>(5)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let (mut budgets, mut entries) = (0, 0);
        for (id, reserved, settled, sum_reserved, sum_settled, count) in rows {
            budgets += 1;
            entries += to_u64(count);
            if reserved != sum_reserved || settled != sum_settled {
                return Ok(LedgerVerdict::Mismatch {
                    budget: id
                        .parse()
                        .map_err(|_| StoreError::Corrupt(format!("bad budget id {id}")))?,
                    detail: format!(
                        "cached reserved/settled {reserved}/{settled}, ledger says {sum_reserved}/{sum_settled}"
                    ),
                });
            }
        }
        Ok(LedgerVerdict::Consistent { budgets, entries })
    }
}

fn read_runs(
    conn: &rusqlite::Connection,
    job: Option<JobId>,
    id: Option<&str>,
) -> Result<Vec<RunRecord>> {
    let sql = "SELECT a.id,a.role,a.provider,a.model,a.status,IFNULL(u.input_tokens,0),IFNULL(u.cached_read,0),IFNULL(u.cached_write,0),IFNULL(u.output_tokens,0),IFNULL(u.provenance,'unavailable'),a.started_at,a.ended_at,CASE WHEN a.started_at IS NOT NULL AND a.ended_at IS NOT NULL THEN CAST(MAX(0,ROUND((julianday(a.ended_at)-julianday(a.started_at))*86400000)) AS INTEGER) END,a.transcript_blob,a.job_id,a.cli_version,(SELECT reason FROM routing_decisions WHERE id=a.routing_decision_id),(SELECT s.content_hash FROM run_capability_snapshots r JOIN capability_snapshots s ON s.id=r.snapshot_id WHERE r.run_id=a.id) FROM agent_runs a LEFT JOIN usage_records u ON u.run_id=a.id WHERE (?1 IS NULL OR a.job_id=?1) AND (?2 IS NULL OR a.id=?2) ORDER BY a.created_at,a.id";
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt
        .query_map(params![job.map(|j| j.to_string()), id], |r| {
            let hash: Option<String> = r.get(13)?;
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, i64>(5)?,
                r.get::<_, i64>(6)?,
                r.get::<_, i64>(7)?,
                r.get::<_, i64>(8)?,
                r.get::<_, String>(9)?,
                r.get::<_, Option<String>>(10)?,
                r.get::<_, Option<String>>(11)?,
                r.get::<_, Option<i64>>(12)?,
                hash,
                r.get::<_, String>(14)?,
                r.get::<_, Option<String>>(15)?,
                r.get::<_, Option<String>>(16)?,
                r.get::<_, Option<String>>(17)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    rows.into_iter()
        .map(|r| {
            Ok(RunRecord {
                id: r.0,
                job_id: r.14,
                cli_version: r.15,
                routing_reason: r.16,
                capability_snapshot_hash: r.17,
                role: r.1,
                provider: r.2,
                model: r.3,
                status: r.4,
                input: to_u64(r.5),
                cache_read: to_u64(r.6),
                cache_write: to_u64(r.7),
                output: to_u64(r.8),
                provenance: r.9,
                started_at: r.10,
                ended_at: r.11,
                duration_ms: r.12.map(to_u64),
                transcript_blob: r
                    .13
                    .map(|h| {
                        h.parse()
                            .map_err(|e| StoreError::Corrupt(format!("bad transcript hash: {e}")))
                    })
                    .transpose()?,
            })
        })
        .collect()
}
