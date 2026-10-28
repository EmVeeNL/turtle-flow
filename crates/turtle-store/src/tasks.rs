//! Plan tasks (`docs/DOMAIN-MODEL.md` §3.3): one row per task of a plan revision, each with its
//! own state machine. Agent runs, gate results, usage and commit records point at their task via
//! `task_id`. A task's id is `<plan revision>/<plan task id>`, the exact value of the
//! `Turtle-Task` trailer on its commit, so Git keeps an independent record that recovery can
//! compare with this table (but never rebuild it from).
//!
//! Every change is a hash-chained `task.*` event carrying the task id.

use std::collections::HashMap;

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use turtle_domain::{Actor, JobId, ProjectId, RunId};

use crate::error::{Result, StoreError};
use crate::store::{NewEvent, Store, Tx, from_s, load_job, to_s};

/// The task state machine. `succeeded` and `skipped` satisfy a dependency (invariant T-2);
/// `succeeded`, `skipped` and `superseded` are final.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    Pending,
    Ready,
    Running,
    Gating,
    Fixing,
    Succeeded,
    Failed,
    Skipped,
    Superseded,
}

impl TaskState {
    /// An agent or the task's gates are working on it.
    #[must_use]
    pub const fn is_active(self) -> bool {
        matches!(self, Self::Running | Self::Gating | Self::Fixing)
    }

    /// Dependents may start (invariant T-2).
    #[must_use]
    pub const fn is_done(self) -> bool {
        matches!(self, Self::Succeeded | Self::Skipped)
    }

    /// The allowed moves. Re-entering `running` from an active state is a retry after a failed
    /// or interrupted attempt; `failed` goes back through `ready` when a human resumes the job.
    #[must_use]
    pub const fn can_become(self, to: Self) -> bool {
        use TaskState as S;
        matches!(
            (self, to),
            (S::Pending, S::Ready | S::Skipped | S::Superseded)
                | (S::Ready, S::Running | S::Skipped | S::Superseded)
                | (
                    S::Running,
                    S::Running | S::Gating | S::Succeeded | S::Failed
                )
                | (S::Gating, S::Running | S::Fixing | S::Succeeded | S::Failed)
                | (S::Fixing, S::Running | S::Gating | S::Failed)
                | (
                    S::Running | S::Gating | S::Fixing | S::Failed,
                    S::Superseded
                )
                | (S::Failed, S::Ready)
        )
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Ready => "ready",
            Self::Running => "running",
            Self::Gating => "gating",
            Self::Fixing => "fixing",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
            Self::Superseded => "superseded",
        }
    }
}

impl std::fmt::Display for TaskState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The id of a plan task's row, and the value of its commit's `Turtle-Task` trailer.
#[must_use]
pub fn task_id(plan_revision_id: &str, plan_task_id: &str) -> String {
    format!("{plan_revision_id}/{plan_task_id}")
}

/// A commit found by its `Turtle-Task` trailer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrailerCommit {
    pub sha: String,
    pub run_id: Option<String>,
}

/// One task of a plan revision, in plan order.
#[derive(Debug, Clone)]
pub struct NewTask {
    pub plan_task_id: String,
    pub title: String,
    pub instructions: String,
    pub expected_paths: Vec<String>,
    pub criteria_refs: Vec<String>,
    /// Plan task ids within the same revision.
    pub depends_on: Vec<String>,
    /// Only for jobs that started before tasks were persisted: a commit that already carries
    /// this task's trailer. The row starts `succeeded` with that commit.
    pub backfill: Option<TrailerCommit>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskRecord {
    pub id: String,
    pub job_id: JobId,
    pub plan_revision_id: String,
    /// The id the planner gave the task (`T1`).
    pub plan_task_id: String,
    /// 1-based position in the plan.
    pub ordinal: u32,
    pub title: String,
    pub instructions: String,
    /// Task row ids.
    pub depends_on: Vec<String>,
    pub state: TaskState,
    pub fix_iterations: u32,
    pub commit_sha: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// A task with what is linked to it, for status displays.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskSummary {
    pub task: TaskRecord,
    pub runs: u32,
    /// The latest result of each gate that ran for the task, in gate order of first use.
    pub gates: Vec<(String, String)>,
}

impl TaskSummary {
    #[must_use]
    pub fn gates_passed(&self) -> usize {
        self.gates.iter().filter(|(_, o)| o == "passed").count()
    }

    #[must_use]
    pub fn gates_failed(&self) -> usize {
        self.gates.iter().filter(|(_, o)| o != "passed").count()
    }
}

/// One per-task gate check, bound to the head and working tree it ran on.
#[derive(Debug, Clone)]
pub struct NewGateResult<'a> {
    pub job_id: JobId,
    pub task_id: &'a str,
    pub gate: &'a str,
    /// `passed`, `failed`, `error` or `skipped`; anything else (a timeout) is stored as `error`.
    pub outcome: &'a str,
    pub command: Option<&'a str>,
    pub exit_code: Option<i64>,
    pub duration_ms: Option<u64>,
    pub head_commit: &'a str,
    pub tree_hash: &'a str,
    pub summary: &'a Value,
}

const SELECT: &str =
    "SELECT id, job_id, plan_revision_id, ordinal, title, instructions, depends_on,
        state, fix_iterations, commit_sha, created_at, updated_at FROM tasks";

/// A task row exactly as stored, before parsing.
struct RawTask {
    id: String,
    job_id: String,
    plan_revision_id: String,
    ordinal: u32,
    title: String,
    instructions: String,
    depends_on: String,
    state: String,
    fix_iterations: u32,
    commit_sha: Option<String>,
    created_at: String,
    updated_at: String,
}

fn map(r: &rusqlite::Row<'_>) -> rusqlite::Result<RawTask> {
    Ok(RawTask {
        id: r.get(0)?,
        job_id: r.get(1)?,
        plan_revision_id: r.get(2)?,
        ordinal: r.get(3)?,
        title: r.get(4)?,
        instructions: r.get(5)?,
        depends_on: r.get(6)?,
        state: r.get(7)?,
        fix_iterations: r.get(8)?,
        commit_sha: r.get(9)?,
        created_at: r.get(10)?,
        updated_at: r.get(11)?,
    })
}

fn build(t: RawTask) -> Result<TaskRecord> {
    Ok(TaskRecord {
        plan_task_id: t
            .id
            .strip_prefix(&format!("{}/", t.plan_revision_id))
            .unwrap_or(&t.id)
            .to_owned(),
        job_id: from_s(&t.job_id)?,
        ordinal: t.ordinal,
        title: t.title,
        instructions: t.instructions,
        depends_on: serde_json::from_str(&t.depends_on)?,
        state: from_s(&t.state)?,
        fix_iterations: t.fix_iterations,
        commit_sha: t.commit_sha,
        created_at: t.created_at,
        updated_at: t.updated_at,
        plan_revision_id: t.plan_revision_id,
        id: t.id,
    })
}

fn query(
    conn: &Connection,
    sql_where: &str,
    args: impl rusqlite::Params,
) -> Result<Vec<TaskRecord>> {
    let mut stmt = conn.prepare(&format!("{SELECT} {sql_where}"))?;
    let rows = stmt.query_map(args, map)?;
    rows.map(|r| build(r?)).collect()
}

fn load_task(conn: &Connection, id: &str) -> Result<TaskRecord> {
    query(conn, "WHERE id = ?1", [id])?
        .into_iter()
        .next()
        .ok_or_else(|| StoreError::NotFound(format!("task {id}")))
}

fn tasks_of_revision(conn: &Connection, plan_revision_id: &str) -> Result<Vec<TaskRecord>> {
    query(
        conn,
        "WHERE plan_revision_id = ?1 ORDER BY ordinal",
        [plan_revision_id],
    )
}

fn tasks_for_job(conn: &Connection, job: JobId) -> Result<Vec<TaskRecord>> {
    query(
        conn,
        "WHERE job_id = ?1 ORDER BY created_at, plan_revision_id, ordinal",
        [job.to_string()],
    )
}

fn gate_outcome(outcome: &str) -> &'static str {
    match outcome {
        "passed" => "passed",
        "failed" => "failed",
        "skipped" => "skipped",
        _ => "error",
    }
}

impl Tx<'_> {
    fn task_event(
        &mut self,
        project: ProjectId,
        task: &TaskRecord,
        kind: &str,
        payload: Value,
        run: Option<&str>,
    ) -> Result<i64> {
        self.append_scoped_event(
            &NewEvent {
                project_id: Some(project),
                job_id: Some(task.job_id),
                kind: kind.into(),
                payload,
                actor: Actor::Scheduler,
            },
            Some(&task.id),
            run,
        )
    }

    /// Persists the tasks of an approved plan revision, once. Calling it again for the same
    /// revision returns the stored rows untouched, whatever their state. Unfinished tasks of
    /// earlier revisions become `superseded`; succeeded ones keep their commits.
    pub fn plan_tasks(
        &mut self,
        job: JobId,
        plan_revision_id: &str,
        tasks: &[NewTask],
    ) -> Result<Vec<TaskRecord>> {
        let existing = tasks_of_revision(&self.tx, plan_revision_id)?;
        if !existing.is_empty() {
            return Ok(existing);
        }
        let project = load_job(&self.tx, job)?.project_id;
        let stale = query(
            &self.tx,
            "WHERE job_id = ?1 AND plan_revision_id <> ?2
               AND state NOT IN ('succeeded','skipped','superseded') ORDER BY created_at, ordinal",
            params![job.to_string(), plan_revision_id],
        )?;
        for task in stale {
            self.move_task(
                project,
                &task,
                TaskState::Superseded,
                None,
                json!({"reason": "plan_revised", "plan_revision": plan_revision_id}),
            )?;
        }
        let backfilled: HashMap<&str, bool> = tasks
            .iter()
            .map(|t| (t.plan_task_id.as_str(), t.backfill.is_some()))
            .collect();
        let now = self.now()?;
        for (ordinal, task) in (1_u32..).zip(tasks) {
            let id = task_id(plan_revision_id, &task.plan_task_id);
            let state = if task.backfill.is_some() {
                TaskState::Succeeded
            } else if task
                .depends_on
                .iter()
                .all(|d| backfilled.get(d.as_str()).copied().unwrap_or(false))
            {
                TaskState::Ready
            } else {
                TaskState::Pending
            };
            let depends_on: Vec<String> = task
                .depends_on
                .iter()
                .map(|d| task_id(plan_revision_id, d))
                .collect();
            self.tx.execute(
                "INSERT INTO tasks(id, job_id, plan_revision_id, ordinal, title, instructions,
                                   expected_paths, criteria_refs, depends_on, state, commit_sha,
                                   created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?12)",
                params![
                    id,
                    job.to_string(),
                    plan_revision_id,
                    ordinal,
                    task.title,
                    task.instructions,
                    serde_json::to_string(&task.expected_paths)?,
                    serde_json::to_string(&task.criteria_refs)?,
                    serde_json::to_string(&depends_on)?,
                    to_s(&state)?,
                    task.backfill.as_ref().map(|c| c.sha.as_str()),
                    now,
                ],
            )?;
            let record = load_task(&self.tx, &id)?;
            let mut payload = json!({
                "task": id,
                "plan_task": task.plan_task_id,
                "plan_revision": plan_revision_id,
                "ordinal": ordinal,
                "state": state,
            });
            if let Some(commit) = &task.backfill {
                payload["source"] = json!("trailer_backfill");
                payload["commit"] = json!(commit.sha);
                self.insert_commit_record(project, &record, &commit.sha, commit.run_id.as_deref())?;
            }
            self.task_event(project, &record, "task.created", payload, None)?;
        }
        tasks_of_revision(&self.tx, plan_revision_id)
    }

    /// Moves a task to `to`, refusing moves the state machine does not allow. Entering `fixing`
    /// counts a fix iteration.
    pub fn transition_task(
        &mut self,
        id: &str,
        to: TaskState,
        run: Option<RunId>,
        detail: Value,
    ) -> Result<TaskRecord> {
        let task = load_task(&self.tx, id)?;
        let project = load_job(&self.tx, task.job_id)?.project_id;
        self.move_task(
            project,
            &task,
            to,
            run.map(|r| r.to_string()).as_deref(),
            detail,
        )
    }

    fn move_task(
        &mut self,
        project: ProjectId,
        task: &TaskRecord,
        to: TaskState,
        run: Option<&str>,
        detail: Value,
    ) -> Result<TaskRecord> {
        if !task.state.can_become(to) {
            return Err(StoreError::InvalidTaskTransition {
                task: task.id.clone(),
                from: task.state.as_str(),
                to: to.as_str(),
            });
        }
        let now = self.now()?;
        let changed = self.tx.execute(
            "UPDATE tasks SET state = ?1, fix_iterations = fix_iterations + ?2, updated_at = ?3
             WHERE id = ?4 AND state = ?5",
            params![
                to.as_str(),
                i64::from(to == TaskState::Fixing),
                now,
                task.id,
                task.state.as_str()
            ],
        )?;
        if changed != 1 {
            return Err(StoreError::Corrupt(format!(
                "task {} changed while it was being moved",
                task.id
            )));
        }
        let moved = load_task(&self.tx, &task.id)?;
        let mut payload = json!({
            "task": task.id,
            "from": task.state,
            "to": to,
            "fix_iterations": moved.fix_iterations,
        });
        if let Some(run) = run {
            payload["run"] = json!(run);
        }
        if let (Value::Object(extra), Value::Object(out)) = (detail, &mut payload) {
            out.extend(extra);
        }
        self.task_event(project, &moved, "task.state_changed", payload, run)?;
        Ok(moved)
    }

    fn insert_commit_record(
        &mut self,
        project: ProjectId,
        task: &TaskRecord,
        sha: &str,
        run: Option<&str>,
    ) -> Result<()> {
        // A trailer may name a run this database never saw (a restored backup); keep the
        // commit record without the dangling link rather than refusing it.
        let run = match run {
            Some(run) => self
                .tx
                .query_row("SELECT id FROM agent_runs WHERE id = ?1", [run], |r| {
                    r.get::<_, String>(0)
                })
                .optional()?,
            None => None,
        };
        let now = self.now()?;
        self.tx.execute(
            "INSERT OR IGNORE INTO commit_records(sha, project_id, job_id, task_id, run_id, kind,
                                                  created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 'task', ?6)",
            params![
                sha,
                project.to_string(),
                task.job_id.to_string(),
                task.id,
                run,
                now
            ],
        )?;
        Ok(())
    }

    /// Records the task's commit (none when its run changed nothing) and marks it succeeded.
    /// Dependents whose dependencies are now all done become `ready`. `recovered` marks a
    /// commit found after a restart: made by this task's own run before the table was updated
    /// (crash boundary 16).
    pub fn record_task_commit(
        &mut self,
        id: &str,
        sha: Option<&str>,
        run: RunId,
        recovered: bool,
    ) -> Result<TaskRecord> {
        let task = load_task(&self.tx, id)?;
        let project = load_job(&self.tx, task.job_id)?.project_id;
        let run_text = run.to_string();
        if let Some(sha) = sha {
            self.tx.execute(
                "UPDATE tasks SET commit_sha = ?1 WHERE id = ?2",
                params![sha, id],
            )?;
            self.insert_commit_record(project, &task, sha, Some(&run_text))?;
        }
        let done = self.move_task(
            project,
            &task,
            TaskState::Succeeded,
            Some(&run_text),
            json!({}),
        )?;
        self.task_event(
            project,
            &done,
            "task.commit_recorded",
            json!({"task": id, "commit": sha, "run": run_text, "recovered": recovered}),
            Some(&run_text),
        )?;
        let siblings = tasks_of_revision(&self.tx, &task.plan_revision_id)?;
        let states: HashMap<&str, TaskState> =
            siblings.iter().map(|t| (t.id.as_str(), t.state)).collect();
        for next in siblings.iter().filter(|t| t.state == TaskState::Pending) {
            if next
                .depends_on
                .iter()
                .all(|d| states.get(d.as_str()).is_some_and(|s| s.is_done()))
            {
                self.move_task(
                    project,
                    next,
                    TaskState::Ready,
                    None,
                    json!({"reason": "dependencies_done"}),
                )?;
            }
        }
        Ok(done)
    }

    /// Stores one per-task gate check in `gate_results`.
    pub fn record_task_gate(&mut self, g: &NewGateResult<'_>) -> Result<()> {
        let now = self.now()?;
        self.tx.execute(
            "INSERT INTO gate_results(id, job_id, task_id, gate, outcome, command, exit_code,
                                      duration_ms, head_commit, tree_hash, summary_json, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                ulid::Ulid::generate().to_string(),
                g.job_id.to_string(),
                g.task_id,
                g.gate,
                gate_outcome(g.outcome),
                g.command,
                g.exit_code,
                g.duration_ms.map(crate::store::db_int).transpose()?,
                g.head_commit,
                g.tree_hash,
                g.summary.to_string(),
                now
            ],
        )?;
        Ok(())
    }

    /// Records that the table and the branch disagree. The event is the audit record; nothing
    /// in the table changes.
    pub fn record_task_mismatch(&mut self, job: JobId, mismatches: &Value) -> Result<i64> {
        let project = load_job(&self.tx, job)?.project_id;
        self.append_event(&NewEvent {
            project_id: Some(project),
            job_id: Some(job),
            kind: "task.mismatch_detected".into(),
            payload: json!({"mismatches": mismatches}),
            actor: Actor::Scheduler,
        })
    }

    pub fn task(&self, id: &str) -> Result<TaskRecord> {
        load_task(&self.tx, id)
    }

    pub fn tasks_of_revision(&self, plan_revision_id: &str) -> Result<Vec<TaskRecord>> {
        tasks_of_revision(&self.tx, plan_revision_id)
    }

    pub fn tasks_for_job(&self, job: JobId) -> Result<Vec<TaskRecord>> {
        tasks_for_job(&self.tx, job)
    }

    /// The task a run belongs to, if any.
    pub fn run_task(&self, run: &str) -> Result<Option<String>> {
        Ok(self
            .tx
            .query_row("SELECT task_id FROM agent_runs WHERE id = ?1", [run], |r| {
                r.get::<_, Option<String>>(0)
            })
            .optional()?
            .flatten())
    }
}

impl Store {
    pub fn tasks_for_job(&self, job: JobId) -> Result<Vec<TaskRecord>> {
        tasks_for_job(self.raw_connection(), job)
    }

    pub fn task(&self, id: &str) -> Result<TaskRecord> {
        load_task(self.raw_connection(), id)
    }

    /// Tasks with their run counts and the latest outcome of each of their gates, for one job
    /// or for every job.
    pub fn task_summaries(&self, job: Option<JobId>) -> Result<Vec<TaskSummary>> {
        let conn = self.raw_connection();
        let job = job.map(|j| j.to_string());
        let tasks = query(
            conn,
            "WHERE (?1 IS NULL OR job_id = ?1) ORDER BY job_id, created_at, plan_revision_id, ordinal",
            [&job],
        )?;
        let mut runs: HashMap<String, u32> = HashMap::new();
        let mut stmt = conn.prepare(
            "SELECT a.task_id, COUNT(*) FROM agent_runs a
             WHERE a.task_id IS NOT NULL AND (?1 IS NULL OR a.job_id = ?1) GROUP BY a.task_id",
        )?;
        for row in stmt.query_map([&job], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, u32>(1)?))
        })? {
            let (task, count) = row?;
            runs.insert(task, count);
        }
        let mut gates: HashMap<String, Vec<(String, String)>> = HashMap::new();
        let mut stmt = conn.prepare(
            "SELECT g.task_id, g.gate, g.outcome FROM gate_results g
             WHERE g.task_id IS NOT NULL AND (?1 IS NULL OR g.job_id = ?1)
               AND g.rowid = (SELECT MAX(h.rowid) FROM gate_results h
                              WHERE h.task_id = g.task_id AND h.gate = g.gate)
             ORDER BY (SELECT MIN(f.rowid) FROM gate_results f
                       WHERE f.task_id = g.task_id AND f.gate = g.gate)",
        )?;
        for row in stmt.query_map([&job], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })? {
            let (task, gate, outcome) = row?;
            gates.entry(task).or_default().push((gate, outcome));
        }
        Ok(tasks
            .into_iter()
            .map(|task| TaskSummary {
                runs: runs.get(&task.id).copied().unwrap_or(0),
                gates: gates.remove(&task.id).unwrap_or_default(),
                task,
            })
            .collect())
    }
}
