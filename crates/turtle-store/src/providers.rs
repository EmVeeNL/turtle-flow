//! Provider limit observations and honest quota-window snapshots.

use rusqlite::params;
use turtle_domain::{RunId, WindowStatus};
use ulid::Ulid;

use crate::clock::format_ts;
use crate::error::Result;
use crate::store::{Store, Tx};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LimitObservation {
    pub id: String,
    pub provider: String,
    pub reset_at: Option<String>,
    pub run_id: Option<String>,
    pub observed_at: String,
    pub raw_message: String,
    pub resolved_at: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct QuotaSnapshot {
    pub provider: String,
    pub observed_at: String,
    pub window_name: String,
    pub utilization: f64,
    pub resets_at: Option<String>,
    pub status: String,
}

impl Tx<'_> {
    pub fn observe_provider_limit(
        &mut self,
        provider: &str,
        run: RunId,
        resets_at: Option<time::OffsetDateTime>,
        raw_message: &str,
    ) -> Result<String> {
        let id = Ulid::generate().to_string();
        let observed_at = self.now()?;
        let resets_at = resets_at.map(format_ts).transpose()?;
        self.tx.execute(
            "INSERT INTO provider_limits_observed(id,provider,run_id,observed_at,resets_at,raw_message)
             VALUES (?1,?2,?3,?4,?5,?6)",
            params![id, provider, run.to_string(), observed_at, resets_at, raw_message],
        )?;
        Ok(id)
    }

    pub fn record_quota_snapshot(
        &mut self,
        provider: &str,
        run: RunId,
        window_name: &str,
        utilization: f64,
        resets_at: Option<i64>,
        status: WindowStatus,
    ) -> Result<()> {
        let id = Ulid::generate().to_string();
        let observed_at = self.now()?;
        let resets_at = resets_at
            .and_then(|seconds| time::OffsetDateTime::from_unix_timestamp(seconds).ok())
            .map(format_ts)
            .transpose()?;
        let status = match status {
            WindowStatus::Allowed => "allowed",
            WindowStatus::Warning => "warning",
            WindowStatus::Rejected => "rejected",
        };
        self.tx.execute(
            "INSERT INTO provider_quota_snapshots(id,provider,observed_at,window_name,utilization,resets_at,status,run_id)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            params![id, provider, observed_at, window_name, utilization, resets_at, status, run.to_string()],
        )?;
        Ok(())
    }

    pub fn resolve_provider_limits(&mut self, provider: &str) -> Result<usize> {
        let now = self.now()?;
        let cutoff = format_ts(self.clock.now() - time::Duration::hours(1))?;
        Ok(self.tx.execute(
            "UPDATE provider_limits_observed SET resolved_at=?1 WHERE provider=?2 AND resolved_at IS NULL
             AND ((resets_at IS NOT NULL AND resets_at <= ?1) OR (resets_at IS NULL AND observed_at <= ?3))",
            params![now, provider, cutoff],
        )?)
    }
}

impl Store {
    pub fn providers_ready_to_clear(&self) -> Result<Vec<String>> {
        let now = format_ts(self.clock.now())?;
        let unknown_cutoff = format_ts(self.clock.now() - time::Duration::hours(1))?;
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT ready.provider FROM provider_limits_observed ready
             WHERE ready.resolved_at IS NULL AND ((ready.resets_at IS NOT NULL AND ready.resets_at <= ?1)
               OR (ready.resets_at IS NULL AND ready.observed_at <= ?2))
               AND NOT EXISTS (SELECT 1 FROM provider_limits_observed pending
                 WHERE pending.provider=ready.provider AND pending.resolved_at IS NULL
                   AND NOT ((pending.resets_at IS NOT NULL AND pending.resets_at <= ?1)
                     OR (pending.resets_at IS NULL AND pending.observed_at <= ?2)))",
        )?;
        let rows = stmt.query_map(params![now, unknown_cutoff], |row| row.get(0))?;
        Ok(rows.collect::<std::result::Result<Vec<String>, _>>()?)
    }

    pub fn current_limited_providers(&self) -> Result<Vec<LimitObservation>> {
        let mut stmt = self.conn.prepare(
            "SELECT id,provider,resets_at,run_id,observed_at,COALESCE(raw_message,''),resolved_at
             FROM provider_limits_observed WHERE resolved_at IS NULL
             ORDER BY observed_at DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(LimitObservation {
                id: row.get(0)?,
                provider: row.get(1)?,
                reset_at: row.get(2)?,
                run_id: row.get(3)?,
                observed_at: row.get(4)?,
                raw_message: row.get(5)?,
                resolved_at: row.get(6)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn latest_quota_snapshots(&self) -> Result<Vec<QuotaSnapshot>> {
        let mut stmt = self.conn.prepare(
            "SELECT provider,observed_at,window_name,utilization,resets_at,status FROM (
               SELECT q.*, ROW_NUMBER() OVER (PARTITION BY provider,window_name ORDER BY observed_at DESC,rowid DESC) AS rank
               FROM provider_quota_snapshots q
             ) WHERE rank=1 ORDER BY provider,window_name",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(QuotaSnapshot {
                provider: row.get(0)?,
                observed_at: row.get(1)?,
                window_name: row.get(2)?,
                utilization: row.get(3)?,
                resets_at: row.get(4)?,
                status: row.get(5)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn limited_provider(&self, provider: &str) -> Result<Option<LimitObservation>> {
        Ok(self
            .current_limited_providers()?
            .into_iter()
            .find(|item| item.provider == provider))
    }
}
