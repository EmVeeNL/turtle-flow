//! One-use dashboard login credentials and bounded command replay records.

use rusqlite::{OptionalExtension, params};
use time::format_description::well_known::Rfc3339;

use crate::{Result, Store, StoreError, Tx};

fn command_replay(conn: &rusqlite::Connection, key: &str) -> Result<Option<(String, String, u64)>> {
    conn.query_row(
        "SELECT job_id, command, resulting_version FROM api_command_keys WHERE key=?1",
        [key],
        |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, i64>(2)?)),
    )
    .optional()
    .map(|row| row.map(|(job, command, version)| (job, command, version as u64)))
    .map_err(Into::into)
}

impl Tx<'_> {
    pub fn command_replay(&self, key: &str) -> Result<Option<(String, String, u64)>> {
        command_replay(&self.tx, key)
    }

    pub fn remember_command(
        &mut self,
        key: &str,
        job_id: &str,
        command: &str,
        version: u64,
    ) -> Result<()> {
        let now = self.now()?;
        self.tx.execute("INSERT INTO api_command_keys(key, job_id, command, resulting_version, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![key, job_id, command, i64::try_from(version).map_err(|_| StoreError::Corrupt("version overflow".into()))?, now])?;
        self.tx.execute("DELETE FROM api_command_keys WHERE key NOT IN (SELECT key FROM api_command_keys ORDER BY created_at DESC, key DESC LIMIT 1024)", [])?;
        Ok(())
    }
}

impl Store {
    /// Only the digest is stored. The caller gives the code to the browser once.
    pub fn create_dashboard_code(&mut self, hash: &str, expires_at: &str) -> Result<()> {
        let now = self
            .now()
            .format(&Rfc3339)
            .map_err(|e| StoreError::Corrupt(e.to_string()))?;
        self.conn.execute(
            "DELETE FROM dashboard_login_codes WHERE used_at IS NOT NULL OR julianday(expires_at)<=julianday(?1)",
            [now],
        )?;
        self.conn.execute(
            "INSERT INTO dashboard_login_codes(code_hash, expires_at) VALUES (?1, ?2)",
            params![hash, expires_at],
        )?;
        Ok(())
    }

    /// An immediate transaction makes redemption atomic across concurrent requests.
    pub fn redeem_dashboard_code(&mut self, hash: &str) -> Result<bool> {
        let now = self
            .now()
            .format(&Rfc3339)
            .map_err(|e| StoreError::Corrupt(e.to_string()))?;
        self.transaction(|tx| {
            let candidate = {
                let mut stmt = tx.tx.prepare("SELECT code_hash FROM dashboard_login_codes WHERE used_at IS NULL AND julianday(expires_at)>julianday(?1)")?;
                let rows = stmt.query_map([&now], |r| r.get::<_, String>(0))?;
                let mut found = None;
                for row in rows {
                    let stored = row?;
                    let mut diff = 0u8;
                    for (a, b) in stored.bytes().zip(hash.bytes()) { diff |= a ^ b; }
                    if stored.len() == hash.len() && diff == 0 { found = Some(stored); }
                }
                found
            };
            let Some(candidate) = candidate else { return Ok(false); };
            let changed = tx.tx.execute("UPDATE dashboard_login_codes SET used_at=?2 WHERE code_hash=?1 AND used_at IS NULL", params![candidate, now])?;
            Ok(changed == 1)
        })
    }

    pub fn command_replay(&self, key: &str) -> Result<Option<(String, String, u64)>> {
        command_replay(&self.conn, key)
    }

    pub fn remember_command(
        &mut self,
        key: &str,
        job_id: &str,
        command: &str,
        version: u64,
    ) -> Result<()> {
        self.transaction(|tx| tx.remember_command(key, job_id, command, version))
    }
}
