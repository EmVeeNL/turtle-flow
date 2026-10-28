//! Plan tasks during `implementation`: the `tasks` table is the record of which task is where,
//! and each task commit's `Turtle-Task` trailer is Git's own record of the same fact.
//!
//! Before any task runs, [`reconcile`] persists the plan's tasks (once per plan revision) and
//! compares the table with the trailers on the job branch. The table is never rebuilt from
//! trailers. Two cases are not disagreements:
//!
//! - a job that started before tasks were persisted has no rows yet; its existing trailer
//!   commits are recorded when the rows are first written (`source: trailer_backfill`);
//! - a commit made by the task's own run (the trailer's `Turtle-Run` belongs to that task in
//!   `agent_runs`) while the table still shows the task active is crash boundary 16: the commit
//!   happened, the database update did not. It is recorded with `recovered: true`.
//!
//! Anything else (a recorded commit missing from the branch, an unrecorded or duplicate commit,
//! a trailer naming a task the plan revision does not have) is reported as a mismatch: the job
//! is blocked with [`BlockReason::TaskStateMismatch`](turtle_workflow::BlockReason) and nothing
//! in the table changes.

use std::collections::HashMap;

use serde_json::{Value, json};
use turtle_domain::{JobId, Plan, RunId};
use turtle_git::Repo;
use turtle_git::tasks::TaskTrailer;
use turtle_store::{
    JobRecord, NewGateResult, NewTask, RevisionRecord, StoreError, TaskRecord, TaskState,
    TrailerCommit, task_id,
};

use crate::dispatcher::ExecError;
use crate::shared::SharedStore;

/// What [`reconcile`] found.
#[derive(Debug)]
pub enum Reconciled {
    /// The current revision's tasks by plan task id; table and branch agree.
    Agreed(HashMap<String, TaskRecord>),
    /// Each entry describes one disagreement; already recorded as `task.mismatch_detected`.
    Mismatch(Vec<Value>),
}

fn store_error(e: StoreError) -> ExecError {
    match e {
        StoreError::InvalidTaskTransition { .. } | StoreError::NotFound(_) => {
            ExecError::Fatal(e.to_string())
        }
        other => ExecError::Retryable(other.to_string()),
    }
}

fn git_error(e: turtle_git::GitError) -> ExecError {
    ExecError::Retryable(e.to_string())
}

/// The plan revision a `Turtle-Task` value belongs to; fix commits (`fix/<run>`) have none.
fn revision_of(task: &str) -> Option<&str> {
    task.split_once('/')
        .map(|(rev, _)| rev)
        .filter(|rev| *rev != "fix")
}

/// Persists the plan's tasks if needed, then checks them against the job branch.
pub async fn reconcile(
    store: &SharedStore,
    repo: &Repo,
    job: &JobRecord,
    base: &str,
    branch: &str,
    plan: &Plan,
    plan_rev: &RevisionRecord,
) -> Result<Reconciled, ExecError> {
    let commits = repo.task_commits(base, branch).await.map_err(git_error)?;
    let job_id = job.job.id;
    let rev = plan_rev.id.clone();
    let new_tasks: Vec<NewTask> = plan
        .tasks
        .iter()
        .map(|t| NewTask {
            plan_task_id: t.id.clone(),
            title: t.title.clone(),
            instructions: t.instructions.clone(),
            expected_paths: t.expected_paths.clone(),
            criteria_refs: t.criteria.clone(),
            depends_on: t.depends_on.clone(),
            backfill: None,
        })
        .collect();
    let trailers = commits.clone();
    let rows = store
        .with(move |s| {
            s.transaction(|tx| {
                if tx.tasks_of_revision(&rev)?.is_empty() {
                    let backfilled: Vec<NewTask> = new_tasks
                        .into_iter()
                        .map(|mut t| {
                            let key = task_id(&rev, &t.plan_task_id);
                            let mut found = trailers.iter().filter(|c| c.task == key);
                            // A duplicated trailer is left for the check below to report.
                            if let (Some(c), None) = (found.next(), found.next()) {
                                t.backfill = Some(TrailerCommit {
                                    sha: c.sha.clone(),
                                    run_id: c.run.clone(),
                                });
                            }
                            t
                        })
                        .collect();
                    tx.plan_tasks(job_id, &rev, &backfilled)?;
                }
                tx.tasks_for_job(job_id)
            })
        })
        .await
        .map_err(store_error)?;

    let (mismatches, recover) = compare(store, &rows, &commits).await?;
    if !mismatches.is_empty() {
        let found = json!(mismatches);
        store
            .with(move |s| s.transaction(|tx| tx.record_task_mismatch(job_id, &found)))
            .await
            .map_err(store_error)?;
        return Ok(Reconciled::Mismatch(mismatches));
    }
    for (task, sha, run) in recover {
        store
            .with(move |s| s.transaction(|tx| tx.record_task_commit(&task, Some(&sha), run, true)))
            .await
            .map_err(store_error)?;
    }
    let rev = plan_rev.id.clone();
    let current = store
        .with(move |s| s.transaction(|tx| tx.tasks_of_revision(&rev)))
        .await
        .map_err(store_error)?;
    Ok(Reconciled::Agreed(
        current
            .into_iter()
            .map(|t| (t.plan_task_id.clone(), t))
            .collect(),
    ))
}

/// Compares every task row of the job with the trailer commits on its branch. Returns the
/// disagreements and the boundary-16 commits to record.
async fn compare(
    store: &SharedStore,
    rows: &[TaskRecord],
    commits: &[TaskTrailer],
) -> Result<(Vec<Value>, Vec<(String, String, RunId)>), ExecError> {
    let by_id: HashMap<&str, &TaskRecord> = rows.iter().map(|t| (t.id.as_str(), t)).collect();
    let mut mismatches = Vec::new();
    let mut recover = Vec::new();
    let mut claimed: HashMap<&str, &str> = HashMap::new();
    for commit in commits {
        let Some(rev) = revision_of(&commit.task) else {
            continue;
        };
        let Some(task) = by_id.get(commit.task.as_str()) else {
            // History from a plan revision that never had rows (a job older than the table)
            // is not this table's to judge.
            if rows.iter().any(|t| t.plan_revision_id == rev) {
                mismatches.push(json!({"kind": "unknown_task", "task": commit.task,
                    "commit": commit.sha}));
            }
            continue;
        };
        if let Some(previous) = claimed.insert(task.id.as_str(), commit.sha.as_str()) {
            mismatches.push(json!({"kind": "duplicate_commit", "task": task.id,
                "commits": [previous, commit.sha]}));
            continue;
        }
        match task.commit_sha.as_deref() {
            Some(sha) if sha == commit.sha => {}
            Some(sha) => mismatches.push(json!({"kind": "commit_differs", "task": task.id,
                "recorded": sha, "found": commit.sha})),
            None => {
                let owner = match commit.run.clone() {
                    Some(run) if task.state.is_active() => store
                        .with(move |s| s.transaction(|tx| tx.run_task(&run)))
                        .await
                        .map_err(store_error)?,
                    _ => None,
                };
                let run = commit.run.as_deref().and_then(|r| r.parse::<RunId>().ok());
                match (owner, run) {
                    (Some(owner), Some(run)) if owner == task.id => {
                        recover.push((task.id.clone(), commit.sha.clone(), run));
                    }
                    _ => mismatches.push(json!({"kind": "unrecorded_commit", "task": task.id,
                        "state": task.state, "commit": commit.sha, "run": commit.run})),
                }
            }
        }
    }
    for task in rows {
        if let Some(sha) = task.commit_sha.as_deref()
            && !commits.iter().any(|c| c.sha == sha && c.task == task.id)
        {
            mismatches.push(json!({"kind": "commit_missing", "task": task.id,
                "recorded": sha}));
        }
    }
    Ok((mismatches, recover))
}

/// Moves a task. A failed task being started again passes through `ready` first.
pub async fn enter(
    store: &SharedStore,
    task: &str,
    to: TaskState,
    run: Option<RunId>,
    detail: Value,
) -> Result<TaskRecord, ExecError> {
    let task = task.to_owned();
    store
        .with(move |s| {
            s.transaction(|tx| {
                let current = tx.task(&task)?;
                if current.state == TaskState::Failed && to == TaskState::Running {
                    tx.transition_task(&task, TaskState::Ready, None, json!({"reason": "retry"}))?;
                }
                tx.transition_task(&task, to, run, detail)
            })
        })
        .await
        .map_err(store_error)
}

/// Records one attempt's gate results against the work tree they checked.
pub async fn record_gates(
    store: &SharedStore,
    wt: &Repo,
    job: JobId,
    task: &str,
    results: &[Value],
) -> Result<(), ExecError> {
    let head = wt.head().await.map_err(git_error)?;
    let tree = wt.working_tree_hash().await.map_err(git_error)?;
    let task = task.to_owned();
    let results = results.to_vec();
    store
        .with(move |s| {
            s.transaction(|tx| {
                for result in &results {
                    let mut summary = result.clone();
                    if let Some(fields) = summary.as_object_mut() {
                        // The output tail stays in the gate result artifact only.
                        fields.remove("output_tail");
                    }
                    tx.record_task_gate(&NewGateResult {
                        job_id: job,
                        task_id: &task,
                        gate: result["gate"].as_str().unwrap_or("unknown"),
                        outcome: result["outcome"].as_str().unwrap_or("error"),
                        command: result["command"].as_str(),
                        exit_code: result["exit_code"].as_i64(),
                        duration_ms: result["duration_ms"].as_u64(),
                        head_commit: &head,
                        tree_hash: &tree,
                        summary: &summary,
                    })?;
                }
                Ok(())
            })
        })
        .await
        .map_err(store_error)
}

/// Records the task's commit (or that its run changed nothing) and marks it succeeded.
pub async fn committed(
    store: &SharedStore,
    task: &str,
    sha: Option<String>,
    run: RunId,
) -> Result<TaskRecord, ExecError> {
    let task = task.to_owned();
    store
        .with(move |s| s.transaction(|tx| tx.record_task_commit(&task, sha.as_deref(), run, false)))
        .await
        .map_err(store_error)
}
