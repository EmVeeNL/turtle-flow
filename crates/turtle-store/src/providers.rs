//! Provider limit observations, honest quota-window snapshots and versioned capability snapshots.

use std::path::Path;

use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use turtle_domain::{RunId, WindowStatus};
use ulid::Ulid;

use crate::chain::hex;
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

/// One persisted capability snapshot (`capability_snapshots`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapabilitySnapshot {
    pub id: String,
    pub provider: String,
    /// Per-provider sequence number, starting at 1.
    pub version: i64,
    pub cli_version: Option<String>,
    pub capabilities: Value,
    pub content_hash: String,
    pub discovered_at: String,
}

/// Hash of what a snapshot asserts: provider, CLI version and capabilities, as canonical JSON.
#[must_use]
pub fn capability_hash(provider: &str, cli_version: Option<&str>, capabilities: &Value) -> String {
    // `json!` builds sorted maps, so the serialization is canonical.
    let doc =
        json!({"provider": provider, "cli_version": cli_version, "capabilities": capabilities});
    hex(Sha256::digest(doc.to_string().as_bytes()))
}

const SELECT_SNAPSHOT: &str =
    "SELECT id,provider,version,cli_version,capabilities,content_hash,discovered_at
     FROM capability_snapshots";

fn read_snapshot(row: &rusqlite::Row<'_>) -> rusqlite::Result<CapabilitySnapshot> {
    let capabilities: String = row.get(4)?;
    Ok(CapabilitySnapshot {
        id: row.get(0)?,
        provider: row.get(1)?,
        version: row.get(2)?,
        cli_version: row.get(3)?,
        capabilities: serde_json::from_str(&capabilities).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(4, rusqlite::types::Type::Text, e.into())
        })?,
        content_hash: row.get(5)?,
        discovered_at: row.get(6)?,
    })
}

/// Newest first, at most `limit`.
fn snapshots(conn: &Connection, provider: &str, limit: u32) -> Result<Vec<CapabilitySnapshot>> {
    let mut stmt = conn.prepare(&format!(
        "{SELECT_SNAPSHOT} WHERE provider=?1 ORDER BY version DESC LIMIT ?2"
    ))?;
    let rows = stmt.query_map(params![provider, limit], read_snapshot)?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

impl Tx<'_> {
    /// Records a snapshot unless it is identical (by hash) to the provider's latest one. Returns
    /// the snapshot in effect and whether it was newly written.
    pub fn record_capability_snapshot(
        &mut self,
        provider: &str,
        cli_version: Option<&str>,
        capabilities: &Value,
    ) -> Result<(CapabilitySnapshot, bool)> {
        let content_hash = capability_hash(provider, cli_version, capabilities);
        let latest = snapshots(&self.tx, provider, 1)?.pop();
        if let Some(latest) = latest.as_ref().filter(|s| s.content_hash == content_hash) {
            return Ok((latest.clone(), false));
        }
        let snapshot = CapabilitySnapshot {
            id: Ulid::generate().to_string(),
            provider: provider.to_owned(),
            version: latest.map_or(1, |s| s.version + 1),
            cli_version: cli_version.map(str::to_owned),
            capabilities: capabilities.clone(),
            content_hash,
            discovered_at: self.now()?,
        };
        self.tx.execute(
            "INSERT INTO capability_snapshots(id,provider,version,cli_version,capabilities,content_hash,discovered_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![snapshot.id, snapshot.provider, snapshot.version, snapshot.cli_version,
                snapshot.capabilities.to_string(), snapshot.content_hash, snapshot.discovered_at],
        )?;
        Ok((snapshot, true))
    }
}

impl Store {
    /// The provider's snapshots, newest first, at most `limit`.
    pub fn capability_snapshots(
        &self,
        provider: &str,
        limit: u32,
    ) -> Result<Vec<CapabilitySnapshot>> {
        snapshots(&self.conn, provider, limit)
    }

    /// The snapshot a run started with, if one was recorded for its provider by then.
    pub fn run_capability_snapshot(&self, run: RunId) -> Result<Option<CapabilitySnapshot>> {
        Ok(self
            .conn
            .query_row(
                &format!(
                    "{SELECT_SNAPSHOT} WHERE id=(SELECT snapshot_id FROM run_capability_snapshots WHERE run_id=?1)"
                ),
                [run.to_string()],
                read_snapshot,
            )
            .optional()?)
    }
}

/// Reads snapshots without creating, migrating or writing the database, for `turtle doctor`.
/// `None` when there is no database yet or its schema predates snapshots.
pub fn read_capability_snapshots(
    db: &Path,
    provider: &str,
    limit: u32,
) -> Result<Option<Vec<CapabilitySnapshot>>> {
    if !db.exists() {
        return Ok(None);
    }
    let conn = Connection::open_with_flags(
        db,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    if crate::migrate::current_version(&conn)? < 8 {
        return Ok(None);
    }
    snapshots(&conn, provider, limit).map(Some)
}
