//! Versioned project resources and SQLite FTS5 retrieval.

use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use turtle_domain::{JobId, ProjectId};
use ulid::Ulid;

use crate::clock::format_ts;
use crate::error::Result;
use crate::store::Store;

#[derive(Debug, Clone)]
pub struct NewResource {
    pub project_id: ProjectId,
    pub title: String,
    pub kind: String,
    pub trust: String,
    pub source: Option<String>,
    pub sha256: String,
    pub media_type: String,
    pub size_bytes: u64,
    pub extracted_text: Option<String>,
    pub job_id: Option<JobId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceRecord {
    pub id: String,
    pub title: String,
    pub kind: String,
    pub trust: String,
    pub version: u32,
    pub sha256: String,
    pub extraction_status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceHit {
    pub resource_id: String,
    pub title: String,
    pub trust: String,
    pub text: String,
    pub rank: f64,
}

impl Store {
    pub fn list_resources(&self, project_id: ProjectId) -> Result<Vec<ResourceRecord>> {
        let mut stmt = self.conn.prepare("SELECT r.id,r.title,r.kind,r.trust_level,v.version,v.sha256,v.extraction_status FROM resources r JOIN resource_versions v ON v.resource_id=r.id WHERE r.project_id=?1 AND v.version=(SELECT MAX(v2.version) FROM resource_versions v2 WHERE v2.resource_id=r.id) ORDER BY r.title,r.id")?;
        let rows = stmt.query_map([project_id.to_string()], |row| {
            Ok(ResourceRecord {
                id: row.get(0)?,
                title: row.get(1)?,
                kind: row.get(2)?,
                trust: row.get(3)?,
                version: row.get(4)?,
                sha256: row.get(5)?,
                extraction_status: row.get(6)?,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    /// Adds a resource version and its text chunks. Identical latest content is idempotent.
    pub fn add_resource(&mut self, new: &NewResource) -> Result<ResourceRecord> {
        let tx = self.conn.transaction()?;
        let project = new.project_id.to_string();
        tx.execute(
            "INSERT OR IGNORE INTO blobs(sha256,size_bytes,media_type,compression,ref_count,created_at) VALUES (?1,?2,?3,'none',1,?4)",
            params![new.sha256, i64::try_from(new.size_bytes).unwrap_or(i64::MAX), new.media_type, format_ts(self.clock.now())?],
        )?;
        let existing: Option<(String, u32, String)> = tx.query_row(
            "SELECT r.id, v.version, v.sha256 FROM resources r JOIN resource_versions v ON v.resource_id=r.id WHERE r.project_id=?1 AND r.title=?2 AND IFNULL(r.source,'')=IFNULL(?3,'') ORDER BY v.version DESC LIMIT 1",
            params![project, new.title, new.source],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        ).optional()?;
        let now = format_ts(self.clock.now())?;
        let (id, version) = match existing {
            Some((id, version, sha)) if sha == new.sha256 => {
                tx.commit()?;
                return Ok(ResourceRecord {
                    id,
                    title: new.title.clone(),
                    kind: new.kind.clone(),
                    trust: new.trust.clone(),
                    version,
                    sha256: sha,
                    extraction_status: if new.extracted_text.is_some() {
                        "done"
                    } else {
                        "unsupported"
                    }
                    .into(),
                });
            }
            Some((id, version, _)) => (id, version + 1),
            None => {
                let id = Ulid::generate().to_string();
                tx.execute("INSERT INTO resources(id,project_id,title,kind,trust_level,source,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?7)", params![id, project, new.title, new.kind, new.trust, new.source, now])?;
                (id, 1)
            }
        };
        let version_id = Ulid::generate().to_string();
        tx.execute("INSERT INTO resource_versions(id,resource_id,version,sha256,media_type,size_bytes,extraction_status,created_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)", params![version_id, id, version, new.sha256, new.media_type, i64::try_from(new.size_bytes).unwrap_or(i64::MAX), if new.extracted_text.is_some() {"done"} else {"unsupported"}, now])?;
        tx.execute("INSERT INTO resource_attachments(resource_id,scope_kind,scope_id,pinned_version_id) VALUES (?1,'project',?2,?3) ON CONFLICT(resource_id,scope_kind,scope_id) DO UPDATE SET pinned_version_id=excluded.pinned_version_id", params![id, project, version_id])?;
        if let Some(job_id) = new.job_id {
            tx.execute("INSERT INTO resource_attachments(resource_id,scope_kind,scope_id,pinned_version_id) VALUES (?1,'job',?2,?3) ON CONFLICT(resource_id,scope_kind,scope_id) DO UPDATE SET pinned_version_id=excluded.pinned_version_id", params![id, job_id.to_string(), version_id])?;
        }
        if let Some(text) = &new.extracted_text {
            let chars: Vec<char> = text.chars().collect();
            for (ordinal, chunk) in chars.chunks(3_500).enumerate() {
                let chunk: String = chunk.iter().collect();
                tx.execute("INSERT INTO chunks(id,source_kind,source_id,project_id,ordinal,heading_path,text,approx_tokens) VALUES (?1,'resource_version',?2,?3,?4,NULL,?5,?6)", params![Ulid::generate().to_string(), version_id, project, i64::try_from(ordinal).unwrap_or(i64::MAX), chunk, chunk.chars().count().div_ceil(4) as i64])?;
            }
        }
        tx.commit()?;
        Ok(ResourceRecord {
            id,
            title: new.title.clone(),
            kind: new.kind.clone(),
            trust: new.trust.clone(),
            version,
            sha256: new.sha256.clone(),
            extraction_status: if new.extracted_text.is_some() {
                "done"
            } else {
                "unsupported"
            }
            .into(),
        })
    }

    /// FTS5 ranked chunks attached to the project, with job attachments included when supplied.
    pub fn search_resources(
        &self,
        project_id: ProjectId,
        job_id: Option<JobId>,
        query: &str,
        limit: usize,
    ) -> Result<Vec<ResourceHit>> {
        let mut stmt = self.conn.prepare("SELECT DISTINCT r.id,r.title,r.trust_level,c.text,bm25(chunks_fts) FROM chunks_fts f JOIN chunks c ON c.rowid=f.rowid JOIN resource_versions v ON v.id=c.source_id JOIN resources r ON r.id=v.resource_id JOIN resource_attachments a ON a.resource_id=r.id AND COALESCE(a.pinned_version_id,v.id)=v.id WHERE f.text MATCH ?1 AND c.project_id=?2 AND c.source_kind='resource_version' AND (a.scope_kind='project' OR (a.scope_kind='job' AND a.scope_id=?3)) ORDER BY bm25(chunks_fts) LIMIT ?4")?;
        let rows = stmt.query_map(
            params![
                query,
                project_id.to_string(),
                job_id.map_or_else(String::new, |id| id.to_string()),
                i64::try_from(limit).unwrap_or(i64::MAX)
            ],
            |row| {
                Ok(ResourceHit {
                    resource_id: row.get(0)?,
                    title: row.get(1)?,
                    trust: row.get(2)?,
                    text: row.get(3)?,
                    rank: row.get(4)?,
                })
            },
        )?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }
}
