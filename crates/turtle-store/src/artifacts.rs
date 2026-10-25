//! Artifact revisions: immutable, numbered, content-addressed by blob hash
//! (`docs/DOMAIN-MODEL.md` §3.5, ADR-013). Blob bytes live in `turtle-artifacts`; this module
//! records which revision of which artifact has which hash.

use rusqlite::{OptionalExtension, params};
use turtle_domain::{Actor, ArtifactId, ArtifactKind, JobId, Sha256};

use crate::error::{Result, StoreError};
use crate::store::{NewEvent, Tx, from_s, to_s};

#[derive(Debug, Clone)]
pub struct NewRevision {
    pub job_id: JobId,
    pub kind: ArtifactKind,
    pub sha256: Sha256,
    pub size_bytes: u64,
    pub media_type: String,
    pub schema_id: Option<String>,
    pub author: Actor,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevisionRecord {
    pub id: String,
    pub artifact_id: ArtifactId,
    pub job_id: JobId,
    pub kind: ArtifactKind,
    pub rev: u32,
    pub sha256: Sha256,
    pub schema_id: Option<String>,
    pub author_kind: String,
    pub author_id: Option<String>,
    pub created_at: String,
}

fn author_parts(actor: &Actor) -> (&'static str, Option<String>) {
    match actor {
        Actor::User { id } => ("user", Some(id.clone())),
        Actor::AgentRun { id } => ("agent_run", Some(id.to_string())),
        Actor::System | Actor::Scheduler => ("system", None),
    }
}

const SELECT: &str = "SELECT r.id, r.artifact_id, a.job_id, a.kind, r.rev, r.sha256, r.schema_id,
        r.author_kind, r.author_id, r.created_at
    FROM artifact_revisions r JOIN artifacts a ON a.id = r.artifact_id";

/// A revision row exactly as stored, before parsing.
struct RawRevision {
    id: String,
    artifact_id: String,
    job_id: String,
    kind: String,
    rev: u32,
    sha256: String,
    schema_id: Option<String>,
    author_kind: String,
    author_id: Option<String>,
    created_at: String,
}

fn map(r: &rusqlite::Row<'_>) -> rusqlite::Result<RawRevision> {
    Ok(RawRevision {
        id: r.get(0)?,
        artifact_id: r.get(1)?,
        job_id: r.get(2)?,
        kind: r.get(3)?,
        rev: r.get(4)?,
        sha256: r.get(5)?,
        schema_id: r.get(6)?,
        author_kind: r.get(7)?,
        author_id: r.get(8)?,
        created_at: r.get(9)?,
    })
}

fn build(t: RawRevision) -> Result<RevisionRecord> {
    Ok(RevisionRecord {
        id: t.id,
        artifact_id: from_s(&t.artifact_id)?,
        job_id: from_s(&t.job_id)?,
        kind: from_s(&t.kind)?,
        rev: t.rev,
        sha256: t
            .sha256
            .parse()
            .map_err(|e| StoreError::Corrupt(format!("bad stored hash: {e}")))?,
        schema_id: t.schema_id,
        author_kind: t.author_kind,
        author_id: t.author_id,
        created_at: t.created_at,
    })
}

pub(crate) fn latest_revision(
    conn: &rusqlite::Connection,
    job: JobId,
    kind: ArtifactKind,
) -> Result<Option<RevisionRecord>> {
    let row = conn
        .query_row(
            &format!("{SELECT} WHERE a.job_id = ?1 AND a.kind = ?2 ORDER BY r.rev DESC LIMIT 1"),
            params![job.to_string(), to_s(&kind)?],
            map,
        )
        .optional()?;
    row.map(build).transpose()
}

pub(crate) fn list_revisions(
    conn: &rusqlite::Connection,
    job: JobId,
    kind: ArtifactKind,
) -> Result<Vec<RevisionRecord>> {
    let mut stmt = conn.prepare(&format!(
        "{SELECT} WHERE a.job_id = ?1 AND a.kind = ?2 ORDER BY r.rev"
    ))?;
    let rows = stmt.query_map(params![job.to_string(), to_s(&kind)?], map)?;
    rows.map(|r| build(r?)).collect()
}

impl Tx<'_> {
    /// Adds a revision to the job's artifact of `kind`, creating the artifact on first use.
    ///
    /// Adding content identical to the latest revision returns that revision unchanged, so a
    /// retried or replayed step cannot create a phantom new revision (which would invalidate
    /// approvals bound to the previous one).
    pub fn add_revision(&mut self, new: &NewRevision) -> Result<RevisionRecord> {
        if let Some(latest) = latest_revision(&self.tx, new.job_id, new.kind)?
            && latest.sha256 == new.sha256
        {
            return Ok(latest);
        }
        let job = self.load_job(new.job_id)?;
        let job_id = new.job_id.to_string();
        let kind = to_s(&new.kind)?;
        let now = self.now()?;

        let artifact_id: String = match self
            .tx
            .query_row(
                "SELECT id FROM artifacts WHERE job_id = ?1 AND kind = ?2",
                params![job_id, kind],
                |r| r.get(0),
            )
            .optional()?
        {
            Some(id) => id,
            None => {
                let id = ArtifactId::new().to_string();
                self.tx.execute(
                    "INSERT INTO artifacts(id, project_id, job_id, kind, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![id, job.project_id.to_string(), job_id, kind, now],
                )?;
                id
            }
        };
        let (prev_id, prev_rev): (Option<String>, u32) = self
            .tx
            .query_row(
                "SELECT id, rev FROM artifact_revisions WHERE artifact_id = ?1 ORDER BY rev DESC LIMIT 1",
                [&artifact_id],
                |r| Ok((Some(r.get(0)?), r.get(1)?)),
            )
            .optional()?
            .unwrap_or((None, 0));

        let size = i64::try_from(new.size_bytes)
            .map_err(|_| StoreError::Corrupt("blob too large".into()))?;
        self.tx.execute(
            "INSERT INTO blobs(sha256, size_bytes, media_type, ref_count, created_at)
             VALUES (?1, ?2, ?3, 1, ?4)
             ON CONFLICT(sha256) DO UPDATE SET ref_count = ref_count + 1",
            params![new.sha256.to_hex(), size, new.media_type, now],
        )?;
        let rev = prev_rev + 1;
        let id = ulid::Ulid::generate().to_string();
        let (author_kind, author_id) = author_parts(&new.author);
        self.tx.execute(
            "INSERT INTO artifact_revisions(id, artifact_id, rev, sha256, schema_id, author_kind,
                                            author_id, parent_rev_id, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                id,
                artifact_id,
                rev,
                new.sha256.to_hex(),
                new.schema_id,
                author_kind,
                author_id,
                prev_id,
                now
            ],
        )?;
        self.append_event(&NewEvent {
            project_id: Some(job.project_id),
            job_id: Some(new.job_id),
            kind: "artifact.revision_added".into(),
            payload: serde_json::json!({
                "artifact": kind, "rev": rev, "sha256": new.sha256.to_string(),
            }),
            actor: new.author.clone(),
        })?;
        Ok(RevisionRecord {
            id,
            artifact_id: from_s(&artifact_id)?,
            job_id: new.job_id,
            kind: new.kind,
            rev,
            sha256: new.sha256,
            schema_id: new.schema_id.clone(),
            author_kind: author_kind.to_owned(),
            author_id,
            created_at: now,
        })
    }

    pub fn latest_revision(
        &self,
        job: JobId,
        kind: ArtifactKind,
    ) -> Result<Option<RevisionRecord>> {
        latest_revision(&self.tx, job, kind)
    }
}

impl crate::store::Store {
    pub fn all_revisions(&self) -> Result<Vec<RevisionRecord>> {
        let mut stmt = self
            .raw_connection()
            .prepare(&format!("{SELECT} ORDER BY r.created_at DESC, r.id DESC"))?;
        let rows = stmt.query_map([], map)?;
        rows.map(|r| build(r?)).collect()
    }

    pub fn revision_by_id(&self, id: &str) -> Result<RevisionRecord> {
        self.raw_connection()
            .query_row(&format!("{SELECT} WHERE r.id=?1"), [id], map)
            .optional()?
            .map(build)
            .transpose()?
            .ok_or_else(|| StoreError::NotFound(format!("artifact revision {id}")))
    }
    pub fn latest_revision(
        &self,
        job: JobId,
        kind: ArtifactKind,
    ) -> Result<Option<RevisionRecord>> {
        latest_revision(self.raw_connection(), job, kind)
    }

    pub fn list_revisions(&self, job: JobId, kind: ArtifactKind) -> Result<Vec<RevisionRecord>> {
        list_revisions(self.raw_connection(), job, kind)
    }

    /// Reference count of a blob (how many revisions point at it).
    pub fn blob_ref_count(&self, hash: &Sha256) -> Result<Option<u32>> {
        Ok(self
            .raw_connection()
            .query_row(
                "SELECT ref_count FROM blobs WHERE sha256 = ?1",
                [hash.to_hex()],
                |r| r.get(0),
            )
            .optional()?)
    }
}
