//! The store: one writer connection, immediate transactions, repositories as methods.
//!
//! Every state change goes through [`Tx::apply_decision`], which writes the job's new state,
//! its events (hash-chained) and its effect intents in **one transaction** with an optimistic
//! version check (ADR-004, ADR-017).

use std::path::Path;
use std::sync::Arc;

use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use sha2::{Digest, Sha256};
use turtle_domain::{
    Actor, EffectId, EventId, JobId, JobKey, JobState, JobType, Priority, ProjectId,
};
use turtle_workflow::{
    BlockInfo, Counters, Decision, Hold, JobAggregate, JobCommand, Profile, WorkflowEvent,
};
use ulid::Ulid;

use crate::chain::{HashedEvent, event_hash, genesis_hash};
use crate::clock::{Clock, SystemClock, format_ts};
use crate::error::{Result, StoreError};
use crate::integrity::{self, Key, KeyStorage};
use crate::migrate;

/// Attempts before an effect is considered failed (retry policy lives in the dispatcher).
const DEFAULT_MAX_ATTEMPTS: u32 = 5;

#[derive(Debug, Clone)]
pub struct NewProject {
    pub slug: String,
    pub key_prefix: String,
    pub name: String,
    pub description: String,
    pub repo_path: String,
    pub default_branch: String,
    pub default_profile: String,
}

#[derive(Debug, Clone)]
pub struct NewJob {
    pub project_id: ProjectId,
    pub title: String,
    pub description: String,
    pub job_type: JobType,
    pub priority: Priority,
    pub profile: Profile,
}

/// A job as stored: the workflow aggregate plus identity and descriptive fields.
#[derive(Debug, Clone, PartialEq)]
pub struct JobRecord {
    pub job: JobAggregate,
    pub project_id: ProjectId,
    pub key: JobKey,
    pub title: String,
    pub description: String,
    pub job_type: JobType,
    pub priority: Priority,
    /// Tip of the target branch when implementation started; fixed once set.
    pub base_commit: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NewEvent {
    pub project_id: Option<ProjectId>,
    pub job_id: Option<JobId>,
    pub kind: String,
    pub payload: Value,
    pub actor: Actor,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventRow {
    pub seq: i64,
    pub id: String,
    pub occurred_at: String,
    pub job_id: Option<String>,
    pub project_id: Option<String>,
    pub kind: String,
    pub payload: Value,
    pub actor_kind: String,
    pub actor_id: String,
    pub prev_hash: String,
    pub hash: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectStatus {
    Pending,
    Claimed,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EffectRow {
    pub id: EffectId,
    pub job_id: Option<JobId>,
    pub kind: String,
    pub idempotency_key: String,
    pub payload: Value,
    pub status: String,
    pub attempts: u32,
    pub max_attempts: u32,
    pub created_event_seq: i64,
}

/// What `apply_decision` wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Applied {
    pub event_seqs: Vec<i64>,
    pub effect_ids: Vec<EffectId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChainVerdict {
    Intact { events: u64, head: String },
    Broken { seq: i64, reason: String },
}

/// Job state reconstructed from events alone, used to prove projections can be rebuilt (NFR-017).
#[derive(Debug, Clone, PartialEq)]
pub struct RebuiltJob {
    pub state: JobState,
    pub holds: Vec<Hold>,
    pub blocked: Option<BlockInfo>,
}

pub struct Store {
    pub(crate) conn: Connection,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) approval_key: Key,
    key_storage: KeyStorage,
}

impl std::fmt::Debug for Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Store").finish_non_exhaustive()
    }
}

impl Store {
    /// Opens (creating if needed) the database at `path`. Parent directories get mode `0700`,
    /// the database file `0600` (SEC-013). Migrates to the latest schema, backing up first.
    pub fn open(path: &Path, clock: Arc<dyn Clock>) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
            set_mode(parent, 0o700)?;
        }
        let mut conn = Connection::open(path)?;
        set_mode(path, 0o600)?;
        conn.pragma_update_and_check(None, "journal_mode", "WAL", |r| r.get::<_, String>(0))?;
        let old_version = migrate::current_version(&conn)?;
        let legacy_key = if old_version > 0 && old_version < 7 {
            let signed: bool = conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM approvals WHERE hmac IS NOT NULL)",
                [],
                |r| r.get(0),
            )?;
            let (key, storage, _) = integrity::load(Some(path), &installation_id(&conn)?, signed)?;
            crate::approvals::migrate_legacy_rows(&mut conn, &*clock, &key)?;
            Some((key, storage))
        } else {
            None
        };
        Self::finish_open(&mut conn, path.parent().map(|p| p.join("backups")), &*clock)?;
        let (approval_key, key_storage) = if let Some(pair) = legacy_key {
            pair
        } else {
            let signed: bool = conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM approvals WHERE hmac IS NOT NULL)",
                [],
                |r| r.get(0),
            )?;
            let (key, storage, _) = integrity::load(Some(path), &installation_id(&conn)?, signed)?;
            (key, storage)
        };
        Ok(Self {
            conn,
            clock,
            approval_key,
            key_storage,
        })
    }

    pub fn open_in_memory(clock: Arc<dyn Clock>) -> Result<Self> {
        let mut conn = Connection::open_in_memory()?;
        Self::finish_open(&mut conn, None, &*clock)?;
        let (approval_key, key_storage, _) =
            integrity::load(None, &installation_id(&conn)?, false)?;
        Ok(Self {
            conn,
            clock,
            approval_key,
            key_storage,
        })
    }

    /// Opens with the system clock.
    pub fn open_default(path: &Path) -> Result<Self> {
        Self::open(path, Arc::new(SystemClock))
    }

    fn finish_open(
        conn: &mut Connection,
        backup_dir: Option<std::path::PathBuf>,
        clock: &dyn Clock,
    ) -> Result<()> {
        // `synchronous=FULL`: approvals and the ledger must survive power loss (NFR-004).
        conn.pragma_update(None, "synchronous", "FULL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        let stamp = format_ts(clock.now())?.replace([':', '.'], "-");
        migrate::migrate(conn, backup_dir.as_deref(), &stamp)?;
        let id = Ulid::generate().to_string();
        conn.execute(
            "INSERT OR IGNORE INTO store_meta(key, value) VALUES ('installation_id', ?1)",
            [id],
        )?;
        Ok(())
    }

    /// Runs `f` in an immediate transaction: committed on `Ok`, rolled back on `Err`.
    pub fn transaction<T>(&mut self, f: impl FnOnce(&mut Tx<'_>) -> Result<T>) -> Result<T> {
        let clock: &dyn Clock = &*self.clock;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut t = Tx {
            tx,
            clock,
            approval_key: &self.approval_key,
        };
        let out = f(&mut t)?;
        t.tx.commit()?;
        Ok(out)
    }

    pub fn installation_id(&self) -> Result<String> {
        installation_id(&self.conn)
    }

    #[must_use]
    pub fn approval_key_storage(&self) -> KeyStorage {
        self.key_storage
    }

    #[must_use]
    pub fn now(&self) -> time::OffsetDateTime {
        self.clock.now()
    }

    pub fn load_job(&self, id: JobId) -> Result<JobRecord> {
        load_job(&self.conn, id)
    }

    pub fn list_events(&self, job: Option<JobId>) -> Result<Vec<EventRow>> {
        list_events(&self.conn, job)
    }

    /// Indexed cursor for live clients. A fixed batch bounds work and memory per connection.
    pub fn events_after(
        &self,
        seq: i64,
        project: Option<&str>,
        job: Option<&str>,
        limit: usize,
    ) -> Result<Vec<EventRow>> {
        let mut stmt = self.conn.prepare("SELECT seq, id, occurred_at, job_id, project_id, type, payload, actor_kind, actor_id, prev_hash, hash FROM events WHERE seq>?1 AND (?2 IS NULL OR project_id=?2) AND (?3 IS NULL OR job_id=?3) ORDER BY seq LIMIT ?4")?;
        let rows = stmt.query_map(params![seq, project, job, limit.min(256) as i64], |r| {
            let payload: String = r.get(6)?;
            Ok(EventRow {
                seq: r.get(0)?,
                id: r.get(1)?,
                occurred_at: r.get(2)?,
                job_id: r.get(3)?,
                project_id: r.get(4)?,
                kind: r.get(5)?,
                payload: serde_json::from_str(&payload).unwrap_or(Value::Null),
                actor_kind: r.get(7)?,
                actor_id: r.get(8)?,
                prev_hash: r.get(9)?,
                hash: r.get(10)?,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    /// True when every job this one depends on is delivered (`done` or `awaiting_release`).
    pub fn dependencies_delivered(&self, job: JobId) -> Result<bool> {
        let open: i64 = self.conn.query_row(
            "SELECT count(*) FROM job_dependencies d JOIN jobs j ON j.id = d.depends_on_id
             WHERE d.job_id = ?1 AND j.state NOT IN ('done', 'awaiting_release')",
            [job.to_string()],
            |r| r.get(0),
        )?;
        Ok(open == 0)
    }

    pub fn list_effects(&self, job: JobId) -> Result<Vec<EffectRow>> {
        list_effects(&self.conn, job)
    }

    /// Walks the whole event log and verifies the hash chain (`turtle audit verify`).
    pub fn verify_chain(&self) -> Result<ChainVerdict> {
        verify_chain(&self.conn)
    }

    pub fn rebuild_job(&self, id: JobId) -> Result<RebuiltJob> {
        rebuild_job(&self.conn, id)
    }

    /// All registered projects, oldest first.
    pub fn list_projects(&self) -> Result<Vec<crate::projects::ProjectRecord>> {
        let ids = self.ids("SELECT id FROM projects ORDER BY created_at, id", [])?;
        ids.into_iter()
            .map(|id| crate::projects::load_project(&self.conn, id))
            .collect()
    }

    pub fn project_by_slug(&self, slug: &str) -> Result<crate::projects::ProjectRecord> {
        let ids: Vec<ProjectId> = self.ids("SELECT id FROM projects WHERE slug = ?1", [slug])?;
        let id = ids
            .into_iter()
            .next()
            .ok_or_else(|| StoreError::NotFound(format!("project `{slug}`")))?;
        crate::projects::load_project(&self.conn, id)
    }

    /// Jobs of one project (or all), newest first.
    pub fn list_jobs(&self, project: Option<ProjectId>) -> Result<Vec<JobRecord>> {
        let ids: Vec<JobId> = match project {
            Some(p) => self.ids(
                "SELECT id FROM jobs WHERE project_id = ?1 ORDER BY created_at DESC, id DESC",
                [p.to_string()],
            )?,
            None => self.ids("SELECT id FROM jobs ORDER BY created_at DESC, id DESC", [])?,
        };
        ids.into_iter().map(|id| load_job(&self.conn, id)).collect()
    }

    /// Looks a job up by its human key (`API-42`).
    pub fn find_job(&self, key: &JobKey) -> Result<JobRecord> {
        let ids: Vec<JobId> = self.ids(
            "SELECT j.id FROM jobs j JOIN projects p ON p.id = j.project_id
             WHERE p.key_prefix = ?1 AND j.number = ?2",
            params![key.prefix(), key.number()],
        )?;
        let id = ids
            .into_iter()
            .next()
            .ok_or_else(|| StoreError::NotFound(format!("job {key}")))?;
        load_job(&self.conn, id)
    }

    fn ids<I: std::str::FromStr>(
        &self,
        sql: &str,
        params: impl rusqlite::Params,
    ) -> Result<Vec<I>> {
        let mut stmt = self.conn.prepare(sql)?;
        let raw = stmt
            .query_map(params, |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        raw.into_iter()
            .map(|s| {
                s.parse()
                    .map_err(|_| StoreError::Corrupt(format!("bad id `{s}`")))
            })
            .collect()
    }

    /// Escape hatch for tests that simulate tampering with the file.
    #[doc(hidden)]
    pub fn raw_connection(&self) -> &Connection {
        &self.conn
    }
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: u32) -> Result<()> {
    Ok(())
}

// ---------------------------------------------------------------- transaction

pub struct Tx<'a> {
    pub(crate) tx: Transaction<'a>,
    pub(crate) clock: &'a dyn Clock,
    pub(crate) approval_key: &'a Key,
}

impl std::fmt::Debug for Tx<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tx").finish_non_exhaustive()
    }
}

impl Tx<'_> {
    pub(crate) fn now(&self) -> Result<String> {
        Ok(format_ts(self.clock.now())?)
    }

    pub fn load_job(&self, id: JobId) -> Result<JobRecord> {
        load_job(&self.tx, id)
    }

    pub fn insert_project(&mut self, p: &NewProject) -> Result<ProjectId> {
        let id = ProjectId::new();
        let now = self.now()?;
        self.tx.execute(
            "INSERT INTO projects(id, slug, key_prefix, name, description, repo_path, default_branch,
                                  default_profile, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)",
            params![
                id.to_string(),
                p.slug,
                p.key_prefix,
                p.name,
                p.description,
                p.repo_path,
                p.default_branch,
                p.default_profile,
                now
            ],
        )?;
        self.append_event(&NewEvent {
            project_id: Some(id),
            job_id: None,
            kind: "project.created".into(),
            payload: serde_json::json!({ "slug": p.slug, "repo_path": p.repo_path }),
            actor: Actor::local_user(),
        })?;
        Ok(id)
    }

    /// Creates a job in `backlog` with the next sequence number of its project.
    pub fn create_job(&mut self, new: &NewJob, actor: &Actor) -> Result<JobRecord> {
        new.profile
            .validate()
            .map_err(|e| StoreError::Corrupt(format!("invalid profile: {e}")))?;
        let project = new.project_id.to_string();
        let (prefix, number): (String, u32) = self
            .tx
            .query_row(
                "SELECT key_prefix, next_job_number FROM projects WHERE id = ?1",
                [&project],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?
            .ok_or_else(|| StoreError::NotFound(format!("project {project}")))?;
        self.tx.execute(
            "UPDATE projects SET next_job_number = next_job_number + 1 WHERE id = ?1",
            [&project],
        )?;
        let key = JobKey::new(prefix, number)
            .map_err(|e| StoreError::Corrupt(format!("bad job key: {e}")))?;
        let job = JobAggregate::new(JobId::new(), new.profile.clone());
        let profile_json = serde_json::to_value(&job.profile)?;
        let profile_sha = hex(Sha256::digest(
            serde_json::to_string(&profile_json)?.as_bytes(),
        ));
        let now = self.now()?;
        self.tx.execute(
            "INSERT INTO jobs(id, project_id, number, title, description, type, priority, state,
                              profile_snapshot, profile_sha, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11)",
            params![
                job.id.to_string(),
                project,
                number,
                new.title,
                new.description,
                to_s(&new.job_type)?,
                to_s(&new.priority)?,
                to_s(&job.state)?,
                profile_json.to_string(),
                profile_sha,
                now
            ],
        )?;
        self.append_event(&NewEvent {
            project_id: Some(new.project_id),
            job_id: Some(job.id),
            kind: "job.created".into(),
            payload: serde_json::json!({ "key": key.to_string(), "title": new.title, "profile": job.profile.name }),
            actor: actor.clone(),
        })?;
        self.load_job(job.id)
    }

    /// Persists the outcome of [`turtle_workflow::decide`]: new job state, events and effect
    /// intents, atomically. Fails with [`StoreError::VersionConflict`] when the job changed
    /// since `expected_version` was read; nothing is written in that case.
    pub fn apply_decision(
        &mut self,
        expected_version: u64,
        decision: &Decision,
        cmd: &JobCommand,
        actor: &Actor,
    ) -> Result<Applied> {
        let job = &decision.job;
        if job.version != expected_version + 1 {
            return Err(StoreError::Corrupt(format!(
                "decision version {} does not follow expected version {expected_version}",
                job.version
            )));
        }
        let id = job.id.to_string();
        let project_id: String = self
            .tx
            .query_row("SELECT project_id FROM jobs WHERE id = ?1", [&id], |r| {
                r.get(0)
            })
            .optional()?
            .ok_or_else(|| StoreError::NotFound(format!("job {id}")))?;
        let project: ProjectId = from_s(&project_id)?;
        let now = self.now()?;

        let changed = self.tx.execute(
            "UPDATE jobs SET state = ?1, holds_json = ?2, blocked_json = ?3, loop_counters = ?4,
                             complexity_class = ?5, version = ?6, updated_at = ?7, ended_at = ?8
             WHERE id = ?9 AND version = ?10",
            params![
                to_s(&job.state)?,
                serde_json::to_string(&job.holds)?,
                job.blocked
                    .as_ref()
                    .map(serde_json::to_string)
                    .transpose()?,
                serde_json::to_string(&job.counters)?,
                job.complexity.as_ref().map(to_s).transpose()?,
                db_int(job.version)?,
                now,
                job.state.is_terminal().then_some(now.as_str()),
                id,
                db_int(expected_version)?
            ],
        )?;
        if changed == 0 {
            return Err(StoreError::VersionConflict {
                job: job.id,
                expected: expected_version,
            });
        }

        let mut event_seqs = Vec::new();
        let mut push = |tx: &mut Self, kind: String, payload: Value| -> Result<()> {
            event_seqs.push(tx.append_event(&NewEvent {
                project_id: Some(project),
                job_id: Some(job.id),
                kind,
                payload,
                actor: actor.clone(),
            })?);
            Ok(())
        };
        push(
            self,
            "job.command".into(),
            serde_json::json!({ "command": cmd.name(), "version": job.version }),
        )?;
        for event in &decision.events {
            let payload = serde_json::to_value(event)?;
            push(self, format!("job.{}", event_tag(&payload)?), payload)?;
        }

        // Eager invalidation: approvals whose assumptions a material change broke are marked
        // in the same transaction. (Guards also recompute binding hashes, so this is defense in
        // depth plus a clear audit trail.)
        for event in &decision.events {
            if let WorkflowEvent::ApprovalInvalidated { gate, change } = event {
                let mut gates = vec![*gate];
                gates.extend(change.invalidates().iter().copied().filter(|g| g != gate));
                self.invalidate_approvals(job.id, &gates, &to_s(change)?)?;
            }
        }

        let created_event_seq = event_seqs[0];
        let mut effect_ids = Vec::new();
        for (idx, effect) in decision.effects.iter().enumerate() {
            let payload = serde_json::to_value(effect)?;
            let kind = payload
                .get("effect")
                .and_then(Value::as_str)
                .ok_or_else(|| StoreError::Corrupt("effect without tag".into()))?
                .to_owned();
            let effect_id = EffectId::new();
            // Deterministic key: replaying the same decision cannot enqueue a second effect.
            let key = format!("job:{id}:v{}:e{idx}", job.version);
            self.tx.execute(
                "INSERT INTO effects(id, job_id, kind, idempotency_key, payload, status, attempts,
                                     max_attempts, created_event_seq, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 'pending', 0, ?6, ?7, ?8)",
                params![
                    effect_id.to_string(),
                    id,
                    kind,
                    key,
                    payload.to_string(),
                    DEFAULT_MAX_ATTEMPTS,
                    created_event_seq,
                    now
                ],
            )?;
            effect_ids.push(effect_id);
        }
        Ok(Applied {
            event_seqs,
            effect_ids,
        })
    }

    /// Appends one event to the hash chain and returns its sequence number.
    pub fn append_event(&mut self, e: &NewEvent) -> Result<i64> {
        self.append_scoped_event(e, None, None)
    }

    /// Appends an event that also names a task and/or run. Both ids are part of the hashed
    /// content, so `audit verify` covers them like every other column.
    pub fn append_scoped_event(
        &mut self,
        e: &NewEvent,
        task_id: Option<&str>,
        run_id: Option<&str>,
    ) -> Result<i64> {
        let prev: String = match self
            .tx
            .query_row(
                "SELECT hash FROM events ORDER BY seq DESC LIMIT 1",
                [],
                |r| r.get(0),
            )
            .optional()?
        {
            Some(h) => h,
            None => genesis_hash(&installation_id(&self.tx)?),
        };
        let id = EventId::new().to_string();
        let occurred_at = self.now()?;
        let (actor_kind, actor_id) = actor_parts(&e.actor);
        let project = e.project_id.map(|p| p.to_string());
        let job = e.job_id.map(|j| j.to_string());
        let hash = event_hash(
            &prev,
            &HashedEvent {
                id: &id,
                occurred_at: &occurred_at,
                project_id: project.as_deref(),
                job_id: job.as_deref(),
                task_id,
                run_id,
                kind: &e.kind,
                payload: &e.payload,
                actor_kind,
                actor_id: &actor_id,
            },
        );
        self.tx.execute(
            "INSERT INTO events(id, occurred_at, project_id, job_id, task_id, run_id, type, payload,
                                actor_kind, actor_id, prev_hash, hash)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                id,
                occurred_at,
                project,
                job,
                task_id,
                run_id,
                e.kind,
                e.payload.to_string(),
                actor_kind,
                actor_id,
                prev,
                hash
            ],
        )?;
        Ok(self.tx.last_insert_rowid())
    }
}

// ---------------------------------------------------------------- reads

fn installation_id(conn: &Connection) -> Result<String> {
    conn.query_row(
        "SELECT value FROM store_meta WHERE key = 'installation_id'",
        [],
        |r| r.get(0),
    )
    .map_err(Into::into)
}

pub(crate) fn load_job(conn: &Connection, id: JobId) -> Result<JobRecord> {
    struct Row {
        project_id: String,
        number: u32,
        title: String,
        description: String,
        job_type: String,
        priority: String,
        state: String,
        holds: String,
        blocked: Option<String>,
        profile: String,
        counters: String,
        complexity: Option<String>,
        version: i64,
        base_commit: Option<String>,
    }
    let prefix_and_row = conn
        .query_row(
            "SELECT p.key_prefix, j.project_id, j.number, j.title, j.description, j.type, j.priority,
                    j.state, j.holds_json, j.blocked_json, j.profile_snapshot, j.loop_counters,
                    j.complexity_class, j.version, j.base_commit
             FROM jobs j JOIN projects p ON p.id = j.project_id WHERE j.id = ?1",
            [id.to_string()],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    Row {
                        project_id: r.get(1)?,
                        number: r.get(2)?,
                        title: r.get(3)?,
                        description: r.get(4)?,
                        job_type: r.get(5)?,
                        priority: r.get(6)?,
                        state: r.get(7)?,
                        holds: r.get(8)?,
                        blocked: r.get(9)?,
                        profile: r.get(10)?,
                        counters: r.get(11)?,
                        complexity: r.get(12)?,
                        version: r.get(13)?,
                        base_commit: r.get(14)?,
                    },
                ))
            },
        )
        .optional()?;
    let (prefix, r) = prefix_and_row.ok_or_else(|| StoreError::NotFound(format!("job {id}")))?;
    let holds: Vec<Hold> = serde_json::from_str(&r.holds)?;
    let job = JobAggregate {
        id,
        state: from_s(&r.state)?,
        // The ready flag and the dependency hold always change together.
        ready_for_implementation: holds.contains(&Hold::WaitingForDependencies),
        holds,
        blocked: r.blocked.as_deref().map(serde_json::from_str).transpose()?,
        profile: serde_json::from_str(&r.profile)?,
        complexity: r.complexity.as_deref().map(from_s).transpose()?,
        counters: serde_json::from_str::<Counters>(&r.counters)?,
        version: u64::try_from(r.version)
            .map_err(|_| StoreError::Corrupt("negative job version".into()))?,
    };
    Ok(JobRecord {
        job,
        project_id: from_s(&r.project_id)?,
        key: JobKey::new(prefix, r.number)
            .map_err(|e| StoreError::Corrupt(format!("bad job key: {e}")))?,
        title: r.title,
        description: r.description,
        job_type: from_s(&r.job_type)?,
        priority: from_s(&r.priority)?,
        base_commit: r.base_commit,
    })
}

fn list_events(conn: &Connection, job: Option<JobId>) -> Result<Vec<EventRow>> {
    let mut stmt = conn.prepare(
        "SELECT seq, id, occurred_at, job_id, project_id, type, payload, actor_kind, actor_id, prev_hash, hash
         FROM events WHERE (?1 IS NULL OR job_id = ?1) ORDER BY seq",
    )?;
    let rows = stmt.query_map([job.map(|j| j.to_string())], |r| {
        let payload: String = r.get(6)?;
        Ok(EventRow {
            seq: r.get(0)?,
            id: r.get(1)?,
            occurred_at: r.get(2)?,
            job_id: r.get(3)?,
            project_id: r.get(4)?,
            kind: r.get(5)?,
            payload: serde_json::from_str(&payload).unwrap_or(Value::Null),
            actor_kind: r.get(7)?,
            actor_id: r.get(8)?,
            prev_hash: r.get(9)?,
            hash: r.get(10)?,
        })
    })?;
    rows.collect::<Result<_, _>>().map_err(Into::into)
}

pub(crate) fn list_effects(conn: &Connection, job: JobId) -> Result<Vec<EffectRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, job_id, kind, idempotency_key, payload, status, attempts, max_attempts,
                created_event_seq
         FROM effects WHERE job_id = ?1 ORDER BY created_event_seq, idempotency_key",
    )?;
    let rows = stmt.query_map([job.to_string()], |r| {
        let id: String = r.get(0)?;
        let job_id: Option<String> = r.get(1)?;
        let payload: String = r.get(4)?;
        Ok(EffectRow {
            id: id.parse().unwrap_or_default(),
            job_id: job_id.and_then(|j| j.parse().ok()),
            kind: r.get(2)?,
            idempotency_key: r.get(3)?,
            payload: serde_json::from_str(&payload).unwrap_or(Value::Null),
            status: r.get(5)?,
            attempts: r.get(6)?,
            max_attempts: r.get(7)?,
            created_event_seq: r.get(8)?,
        })
    })?;
    rows.collect::<Result<_, _>>().map_err(Into::into)
}

fn verify_chain(conn: &Connection) -> Result<ChainVerdict> {
    let mut expected_prev = genesis_hash(&installation_id(conn)?);
    let mut stmt = conn.prepare(
        "SELECT seq, id, occurred_at, project_id, job_id, task_id, run_id, type, payload,
                actor_kind, actor_id, prev_hash, hash FROM events ORDER BY seq",
    )?;
    let mut rows = stmt.query([])?;
    let mut count = 0u64;
    while let Some(r) = rows.next()? {
        let seq: i64 = r.get(0)?;
        let stored_prev: String = r.get(11)?;
        let stored_hash: String = r.get(12)?;
        if stored_prev != expected_prev {
            return Ok(ChainVerdict::Broken {
                seq,
                reason: "previous-hash link does not match the preceding event".into(),
            });
        }
        let payload_text: String = r.get(8)?;
        let Ok(payload) = serde_json::from_str::<Value>(&payload_text) else {
            return Ok(ChainVerdict::Broken {
                seq,
                reason: "payload is not valid JSON".into(),
            });
        };
        let (id, at, kind, actor_kind, actor_id): (String, String, String, String, String) =
            (r.get(1)?, r.get(2)?, r.get(7)?, r.get(9)?, r.get(10)?);
        let (project, job, task, run): (
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
        ) = (r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?);
        let recomputed = event_hash(
            &stored_prev,
            &HashedEvent {
                id: &id,
                occurred_at: &at,
                project_id: project.as_deref(),
                job_id: job.as_deref(),
                task_id: task.as_deref(),
                run_id: run.as_deref(),
                kind: &kind,
                payload: &payload,
                actor_kind: &actor_kind,
                actor_id: &actor_id,
            },
        );
        if recomputed != stored_hash {
            return Ok(ChainVerdict::Broken {
                seq,
                reason: "event content does not match its hash".into(),
            });
        }
        expected_prev = stored_hash;
        count += 1;
    }
    Ok(ChainVerdict::Intact {
        events: count,
        head: expected_prev,
    })
}

fn rebuild_job(conn: &Connection, id: JobId) -> Result<RebuiltJob> {
    let mut view = RebuiltJob {
        state: JobState::Backlog,
        holds: Vec::new(),
        blocked: None,
    };
    for e in list_events(conn, Some(id))? {
        match e.kind.as_str() {
            "job.stage_entered" => {
                view.state =
                    from_s(e.payload["to"].as_str().ok_or_else(|| {
                        StoreError::Corrupt("stage_entered without target".into())
                    })?)?;
                if view.state != JobState::Blocked {
                    view.blocked = None;
                }
            }
            "job.hold_added" => view
                .holds
                .push(serde_json::from_value(e.payload["hold"].clone())?),
            "job.hold_removed" => {
                let hold: Hold = serde_json::from_value(e.payload["hold"].clone())?;
                if let Some(i) = view.holds.iter().position(|h| *h == hold) {
                    view.holds.remove(i);
                }
            }
            "job.blocked" => {
                view.blocked = Some(serde_json::from_value(e.payload["info"].clone())?)
            }
            _ => {}
        }
    }
    Ok(view)
}

// ---------------------------------------------------------------- helpers

fn actor_parts(actor: &Actor) -> (&'static str, String) {
    match actor {
        Actor::User { id } => ("user", id.clone()),
        Actor::System => ("system", "system".into()),
        Actor::Scheduler => ("scheduler", "scheduler".into()),
        Actor::AgentRun { id } => ("agent_run", id.to_string()),
    }
}

fn event_tag(payload: &Value) -> Result<&str> {
    payload
        .get("event")
        .and_then(Value::as_str)
        .ok_or_else(|| StoreError::Corrupt("workflow event without tag".into()))
}

pub(crate) fn to_s<T: Serialize>(v: &T) -> Result<String> {
    match serde_json::to_value(v)? {
        Value::String(s) => Ok(s),
        other => Err(StoreError::Corrupt(format!(
            "expected a string enum, got {other}"
        ))),
    }
}

pub(crate) fn from_s<T: DeserializeOwned>(s: &str) -> Result<T> {
    Ok(serde_json::from_value(Value::String(s.to_owned()))?)
}

/// SQLite integers are signed 64-bit.
pub(crate) fn db_int(v: u64) -> Result<i64> {
    i64::try_from(v)
        .map_err(|_| StoreError::Corrupt(format!("{v} does not fit a database integer")))
}

fn hex(bytes: impl AsRef<[u8]>) -> String {
    bytes.as_ref().iter().map(|b| format!("{b:02x}")).collect()
}
