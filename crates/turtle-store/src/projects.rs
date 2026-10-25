//! Project records and per-job facts the executors persist: base commit, deliveries, and
//! superseding approvals when a job ends.

use rusqlite::{OptionalExtension, params};
use turtle_domain::{Actor, ApprovalId, JobId, ProjectId, ProjectSettings};

use crate::error::{Result, StoreError};
use crate::store::{NewEvent, Tx};

#[derive(Debug, Clone, PartialEq)]
pub struct ProjectRecord {
    pub id: ProjectId,
    pub slug: String,
    pub key_prefix: String,
    pub name: String,
    pub description: String,
    pub status: String,
    pub repo_path: String,
    pub default_branch: String,
    pub default_profile: String,
    pub settings: ProjectSettings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliveryStatus {
    Delivered,
    PendingIntegration,
}

impl DeliveryStatus {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Delivered => "delivered",
            Self::PendingIntegration => "pending_integration",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeliveryRecord {
    pub job_id: JobId,
    pub mode: String,
    pub target_branch: String,
    pub result_commit: Option<String>,
    pub status: String,
    pub approval_id: String,
}

pub(crate) fn load_project(conn: &rusqlite::Connection, id: ProjectId) -> Result<ProjectRecord> {
    let row = conn
        .query_row(
            "SELECT slug, key_prefix, name, repo_path, default_branch, settings_json, default_profile, description, status
             FROM projects WHERE id = ?1",
            [id.to_string()],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, String>(6)?,
                    r.get::<_, String>(7)?,
                    r.get::<_, String>(8)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| StoreError::NotFound(format!("project {id}")))?;
    Ok(ProjectRecord {
        id,
        slug: row.0,
        key_prefix: row.1,
        name: row.2,
        description: row.7,
        status: row.8,
        repo_path: row.3,
        default_branch: row.4,
        default_profile: row.6,
        settings: serde_json::from_str(&row.5)?,
    })
}

pub(crate) fn delivery(conn: &rusqlite::Connection, job: JobId) -> Result<Option<DeliveryRecord>> {
    Ok(conn
        .query_row(
            "SELECT mode, target_branch, result_commit, status, approval_id FROM delivery_records
             WHERE job_id = ?1 ORDER BY created_at DESC, id DESC LIMIT 1",
            [job.to_string()],
            |r| {
                Ok(DeliveryRecord {
                    job_id: job,
                    mode: r.get(0)?,
                    target_branch: r.get(1)?,
                    result_commit: r.get(2)?,
                    status: r.get(3)?,
                    approval_id: r.get(4)?,
                })
            },
        )
        .optional()?)
}

impl Tx<'_> {
    pub fn load_project(&self, id: ProjectId) -> Result<ProjectRecord> {
        load_project(&self.tx, id)
    }

    pub fn update_project_settings(
        &mut self,
        id: ProjectId,
        settings: &ProjectSettings,
    ) -> Result<()> {
        let now = self.now()?;
        let n = self.tx.execute(
            "UPDATE projects SET settings_json = ?1, updated_at = ?2 WHERE id = ?3",
            params![serde_json::to_string(settings)?, now, id.to_string()],
        )?;
        if n == 1 {
            Ok(())
        } else {
            Err(StoreError::NotFound(format!("project {id}")))
        }
    }

    /// Records the commit implementation started from. Idempotent for the same value; changing
    /// it later is refused, because approvals and diffs are bound to it.
    pub fn set_base_commit(&mut self, job: JobId, commit: &str) -> Result<()> {
        let existing: Option<String> = self
            .tx
            .query_row(
                "SELECT base_commit FROM jobs WHERE id = ?1",
                [job.to_string()],
                |r| r.get(0),
            )
            .optional()?
            .ok_or_else(|| StoreError::NotFound(format!("job {job}")))?;
        match existing {
            Some(c) if c == commit => Ok(()),
            Some(c) => Err(StoreError::Corrupt(format!(
                "base commit of job {job} is already {c}, refusing to change it to {commit}"
            ))),
            None => {
                self.tx.execute(
                    "UPDATE jobs SET base_commit = ?1 WHERE id = ?2",
                    params![commit, job.to_string()],
                )?;
                Ok(())
            }
        }
    }

    /// Records the outcome of delivering a job. Idempotent: a job already delivered or parked
    /// for integration keeps its first record.
    pub fn record_delivery(
        &mut self,
        job: JobId,
        mode: &str,
        target_branch: &str,
        result_commit: &str,
        status: DeliveryStatus,
        approval: ApprovalId,
    ) -> Result<()> {
        if delivery(&self.tx, job)?.is_some() {
            return Ok(());
        }
        let now = self.now()?;
        self.tx.execute(
            "INSERT INTO delivery_records(id, job_id, mode, target_branch, result_commit, status,
                                          approval_id, created_at, completed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
            params![
                ulid::Ulid::generate().to_string(),
                job.to_string(),
                mode,
                target_branch,
                result_commit,
                status.as_str(),
                approval.to_string(),
                now
            ],
        )?;
        let rec = self.load_job(job)?;
        self.append_event(&NewEvent {
            project_id: Some(rec.project_id),
            job_id: Some(job),
            kind: "job.delivery_recorded".into(),
            payload: serde_json::json!({
                "mode": mode, "target_branch": target_branch,
                "commit": result_commit, "status": status.as_str(),
            }),
            actor: Actor::Scheduler,
        })?;
        Ok(())
    }

    /// Marks every pending approval of the job as superseded (the job ended).
    pub fn supersede_pending_approvals(&mut self, job: JobId) -> Result<u32> {
        let rec = self.load_job(job)?;
        let ids: Vec<String> = {
            let mut stmt = self
                .tx
                .prepare("SELECT id FROM approvals WHERE job_id = ?1 AND status = 'pending'")?;
            let rows = stmt.query_map([job.to_string()], |r| r.get(0))?;
            rows.collect::<Result<_, _>>()?
        };
        for id in &ids {
            self.tx.execute(
                "UPDATE approvals SET status = 'superseded' WHERE id = ?1",
                [id],
            )?;
            self.append_event(&NewEvent {
                project_id: Some(rec.project_id),
                job_id: Some(job),
                kind: "approval.superseded".into(),
                payload: serde_json::json!({ "approval": id, "reason": "job_ended" }),
                actor: Actor::Scheduler,
            })?;
        }
        Ok(u32::try_from(ids.len()).unwrap_or(u32::MAX))
    }
}

impl crate::store::Store {
    pub fn load_project(&self, id: ProjectId) -> Result<ProjectRecord> {
        load_project(self.raw_connection(), id)
    }

    pub fn delivery(&self, job: JobId) -> Result<Option<DeliveryRecord>> {
        delivery(self.raw_connection(), job)
    }

    pub fn project_of_job(&self, job: JobId) -> Result<ProjectRecord> {
        let rec = self.load_job(job)?;
        load_project(self.raw_connection(), rec.project_id)
    }
}
