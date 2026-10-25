//! Persistent sufficient statistics for reported token usage.

use rusqlite::{OptionalExtension, params};
use turtle_domain::RunId;
use turtle_tokens::{BucketPath, BucketStats};

use crate::error::{Result, StoreError};
use crate::store::{Store, Tx, db_int};

fn read_bucket(conn: &rusqlite::Connection, key: &str) -> Result<Option<BucketStats>> {
    let row: Option<(i64, f64, f64, String)> = conn
        .query_row(
            "SELECT n, mean_ln, m2_ln, reservoir FROM estimation_buckets WHERE bucket_key = ?1",
            [key],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?;
    row.map(|(n, mean_ln, m2_ln, reservoir)| {
        let reservoir = serde_json::from_str(&reservoir)
            .map_err(|e| StoreError::Corrupt(format!("invalid estimation reservoir: {e}")))?;
        Ok(BucketStats {
            n: u64::try_from(n).map_err(|_| StoreError::Corrupt("negative bucket count".into()))?,
            mean_ln,
            m2_ln,
            reservoir,
        })
    })
    .transpose()
}

impl Tx<'_> {
    pub fn estimation_buckets(&self, path: &BucketPath) -> Result<[Option<BucketStats>; 4]> {
        let mut out = [None, None, None, None];
        for (i, key) in path.0.iter().enumerate() {
            out[i] = read_bucket(&self.tx, key)?;
        }
        Ok(out)
    }

    /// A reported run contributes once, including without a budget. Call in the settlement transaction.
    pub fn settle_with_estimation(
        &mut self,
        run: RunId,
        actual: u64,
        path: &BucketPath,
    ) -> Result<bool> {
        let settled = self.settle(run, actual)?;
        if actual == 0 {
            return Ok(settled);
        }
        let provenance: Option<String> = self
            .tx
            .query_row(
                "SELECT provenance FROM usage_records WHERE run_id = ?1",
                [run.to_string()],
                |r| r.get(0),
            )
            .optional()?;
        if provenance.as_deref() != Some("reported") {
            return Ok(settled);
        }
        let now = self.now()?;
        let fresh = self.tx.execute(
            "INSERT OR IGNORE INTO estimation_observations(run_id, observed_at) VALUES (?1,?2)",
            params![run.to_string(), now],
        )?;
        if fresh == 0 {
            return Ok(settled);
        }
        for (level, key) in path.0.iter().enumerate() {
            let mut bucket = read_bucket(&self.tx, key)?.unwrap_or_default();
            bucket.observe(actual);
            self.tx.execute(
                "INSERT INTO estimation_buckets(bucket_key,level,n,mean_ln,m2_ln,reservoir,updated_at)
                 VALUES (?1,?2,?3,?4,?5,?6,?7)
                 ON CONFLICT(bucket_key) DO UPDATE SET n=excluded.n,mean_ln=excluded.mean_ln,
                     m2_ln=excluded.m2_ln,reservoir=excluded.reservoir,updated_at=excluded.updated_at",
                params![key, db_int(level as u64)?, db_int(bucket.n)?, bucket.mean_ln, bucket.m2_ln,
                    serde_json::to_string(&bucket.reservoir)?, now],
            )?;
        }
        Ok(true)
    }
}

impl Store {
    pub fn estimation_buckets(&self, path: &BucketPath) -> Result<[Option<BucketStats>; 4]> {
        let mut out = [None, None, None, None];
        for (i, key) in path.0.iter().enumerate() {
            out[i] = read_bucket(self.raw_connection(), key)?;
        }
        Ok(out)
    }
}
