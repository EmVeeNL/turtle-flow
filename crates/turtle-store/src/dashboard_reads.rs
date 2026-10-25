//! Dashboard projections backed by the existing normalized tables.
use crate::{JobUsage, Result, Store};
use rusqlite::{OptionalExtension, params};
use turtle_domain::{JobId, ProjectId};

#[derive(Debug, Clone)]
pub struct TaskRow {
    pub id: String,
    pub job_id: String,
    pub ordinal: i64,
    pub title: String,
    pub state: String,
    pub commit_sha: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ProjectStats {
    pub gate_pass_rate: Option<f64>,
    pub last_delivery: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DashboardEstimate {
    pub p10: u64,
    pub p50: u64,
    pub p90: u64,
    pub confidence: String,
}

impl Store {
    pub fn dashboard_latest_provider(&self, job: JobId) -> Result<Option<String>> {
        self.conn.query_row(
            "SELECT provider FROM agent_runs WHERE job_id=?1 ORDER BY created_at DESC,id DESC LIMIT 1",
            [job.to_string()], |r|r.get(0)
        ).optional().map_err(Into::into)
    }

    pub fn dashboard_estimate(&self, job: JobId) -> Result<Option<DashboardEstimate>> {
        self.conn.query_row(
            "SELECT p10,p50,p90,confidence FROM preflight_reports WHERE job_id=?1 AND p10 IS NOT NULL AND p50 IS NOT NULL AND p90 IS NOT NULL ORDER BY created_at DESC,id DESC LIMIT 1",
            [job.to_string()], |r|Ok(DashboardEstimate {
                p10:r.get::<_,i64>(0)?.max(0) as u64,
                p50:r.get::<_,i64>(1)?.max(0) as u64,
                p90:r.get::<_,i64>(2)?.max(0) as u64,
                confidence:r.get::<_,Option<String>>(3)?.unwrap_or_else(||"unknown".into())
            })
        ).optional().map_err(Into::into)
    }
    pub fn dashboard_job_usage(&self, job: JobId) -> Result<JobUsage> {
        let mut usage = self.job_usage(job)?;
        let missing: i64=self.conn.query_row(
            "SELECT COUNT(*) FROM agent_runs a LEFT JOIN usage_records u ON u.run_id=a.id WHERE a.job_id=?1 AND u.id IS NULL",
            [job.to_string()], |r|r.get(0)
        )?;
        usage.runs += missing.max(0) as u64;
        usage.runs_unavailable += missing.max(0) as u64;
        Ok(usage)
    }
    pub fn dashboard_project_stats(&self, project: ProjectId) -> Result<ProjectStats> {
        let (passed,total): (i64,i64)=self.conn.query_row(
            "SELECT COUNT(*) FILTER (WHERE g.outcome='passed'), COUNT(*) FROM gate_results g JOIN jobs j ON j.id=g.job_id WHERE j.project_id=?1 AND g.outcome IN ('passed','failed','error')",
            [project.to_string()], |r| Ok((r.get(0)?,r.get(1)?))
        )?;
        let last_delivery=self.conn.query_row(
            "SELECT MAX(d.completed_at) FROM delivery_records d JOIN jobs j ON j.id=d.job_id WHERE j.project_id=?1 AND d.status='delivered'",
            [project.to_string()], |r| r.get(0)
        )?;
        Ok(ProjectStats {
            gate_pass_rate: (total > 0).then_some(100.0 * passed as f64 / total as f64),
            last_delivery,
        })
    }
    pub fn dashboard_tasks(&self, job: Option<JobId>) -> Result<Vec<TaskRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, job_id, ordinal, title, state, commit_sha FROM tasks
             WHERE (?1 IS NULL OR job_id=?1) ORDER BY job_id, ordinal",
        )?;
        let rows = stmt.query_map(params![job.map(|j| j.to_string())], |r| {
            Ok(TaskRow {
                id: r.get(0)?,
                job_id: r.get(1)?,
                ordinal: r.get(2)?,
                title: r.get(3)?,
                state: r.get(4)?,
                commit_sha: r.get(5)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn recent_events(&self, limit: usize) -> Result<Vec<crate::EventRow>> {
        let latest: Option<i64> = self
            .conn
            .query_row(
                "SELECT seq FROM events ORDER BY seq DESC LIMIT 1",
                [],
                |r| r.get(0),
            )
            .optional()?;
        let Some(latest) = latest else {
            return Ok(Vec::new());
        };
        // For a dashboard preview, start close to the tail then trim to the requested size.
        let mut rows = self.events_after(
            latest.saturating_sub(limit.min(256) as i64),
            None,
            None,
            limit,
        )?;
        rows.reverse();
        Ok(rows)
    }
}
