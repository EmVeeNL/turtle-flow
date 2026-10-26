//! Approvals bound to content hashes (`docs/WORKFLOW.md` §6, ADR-006).
//!
//! An approval row stores the binding document and its hash. Deciding requires echoing that
//! hash; the engine additionally recomputes the expected hash from current state whenever a
//! guard needs the approval, so a stale approval can never satisfy a guard.

use rusqlite::{OptionalExtension, params};
use turtle_domain::{Actor, ApprovalId, ApprovalStatus, GateKind, JobId, Sha256};
use turtle_policy::ApprovalBinding;

use crate::error::{Result, StoreError};
use crate::integrity::{self, Key};
use crate::store::{NewEvent, Tx, from_s, to_s};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Approve,
    RequestChanges,
    Reject,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ApprovalRecord {
    pub id: ApprovalId,
    pub job_id: JobId,
    pub gate: GateKind,
    pub binding: ApprovalBinding,
    pub subject_hash: Sha256,
    pub status: ApprovalStatus,
    pub decided_by: Option<String>,
    pub decided_at: Option<String>,
    pub channel: Option<String>,
    pub comment: Option<String>,
    pub invalidated_reason: Option<String>,
    pub created_at: String,
}

const SELECT: &str = "SELECT id, job_id, gate_kind, binding_json, subject_hash, status, decided_by,
    decided_at, channel, comment, invalidated_reason, created_at, hmac, release_id,
    auto_rule_id, expires_at FROM approvals";

/// An approval row exactly as stored, before parsing.
struct RawApproval {
    id: String,
    job_id: String,
    gate: String,
    binding: String,
    subject_hash: String,
    status: String,
    decided_by: Option<String>,
    decided_at: Option<String>,
    channel: Option<String>,
    comment: Option<String>,
    invalidated_reason: Option<String>,
    created_at: String,
    hmac: Option<String>,
    release_id: Option<String>,
    auto_rule_id: Option<String>,
    expires_at: Option<String>,
}

fn map(r: &rusqlite::Row<'_>) -> rusqlite::Result<RawApproval> {
    Ok(RawApproval {
        id: r.get(0)?,
        job_id: r.get(1)?,
        gate: r.get(2)?,
        binding: r.get(3)?,
        subject_hash: r.get(4)?,
        status: r.get(5)?,
        decided_by: r.get(6)?,
        decided_at: r.get(7)?,
        channel: r.get(8)?,
        comment: r.get(9)?,
        invalidated_reason: r.get(10)?,
        created_at: r.get(11)?,
        hmac: r.get(12)?,
        release_id: r.get(13)?,
        auto_rule_id: r.get(14)?,
        expires_at: r.get(15)?,
    })
}

fn signature(t: &RawApproval, key: &Key) -> Result<String> {
    let fields = serde_json::json!([
        t.id,
        t.job_id,
        t.gate,
        t.binding,
        t.subject_hash,
        t.status,
        t.decided_by,
        t.decided_at,
        t.channel,
        t.comment,
        t.invalidated_reason,
        t.created_at,
        t.release_id,
        t.auto_rule_id,
        t.expires_at
    ]);
    Ok(integrity::mac(
        key,
        serde_json::to_string(&fields)?.as_bytes(),
    ))
}

fn build(t: RawApproval, key: &Key) -> Result<ApprovalRecord> {
    let expected = signature(&t, key)?;
    if !t
        .hmac
        .as_ref()
        .is_some_and(|h| integrity::matches(h, &expected))
    {
        return Err(StoreError::ApprovalIntegrity {
            id: t.id,
            job: t.job_id,
        });
    }
    Ok(ApprovalRecord {
        id: t
            .id
            .parse()
            .map_err(|e| StoreError::Corrupt(format!("bad approval id: {e}")))?,
        job_id: t
            .job_id
            .parse()
            .map_err(|e| StoreError::Corrupt(format!("bad job id: {e}")))?,
        gate: from_s(&t.gate)?,
        binding: serde_json::from_str(&t.binding)?,
        subject_hash: t
            .subject_hash
            .parse()
            .map_err(|e| StoreError::Corrupt(format!("bad hash: {e}")))?,
        status: from_s(&t.status)?,
        decided_by: t.decided_by,
        decided_at: t.decided_at,
        channel: t.channel,
        comment: t.comment,
        invalidated_reason: t.invalidated_reason,
        created_at: t.created_at,
    })
}

pub(crate) fn approval(
    conn: &rusqlite::Connection,
    key: &Key,
    id: ApprovalId,
) -> Result<ApprovalRecord> {
    conn.query_row(&format!("{SELECT} WHERE id = ?1"), [id.to_string()], map)
        .optional()?
        .map(|r| build(r, key))
        .transpose()?
        .ok_or_else(|| StoreError::NotFound(format!("approval {id}")))
}

pub(crate) fn latest_with_status(
    conn: &rusqlite::Connection,
    key: &Key,
    job: JobId,
    gate: GateKind,
    status: ApprovalStatus,
) -> Result<Option<ApprovalRecord>> {
    conn.query_row(
        &format!("{SELECT} WHERE job_id = ?1 AND gate_kind = ?2 AND status = ?3 ORDER BY created_at DESC, id DESC LIMIT 1"),
        params![job.to_string(), to_s(&gate)?, to_s(&status)?],
        map,
    )
    .optional()?
    .map(|r| build(r, key))
    .transpose()
}

pub(crate) fn pending(conn: &rusqlite::Connection, key: &Key) -> Result<Vec<ApprovalRecord>> {
    let mut stmt = conn.prepare(&format!(
        "{SELECT} WHERE status = 'pending' ORDER BY created_at, id"
    ))?;
    let rows = stmt.query_map([], map)?;
    rows.map(|r| build(r?, key)).collect()
}

pub(crate) fn list_for_job(
    conn: &rusqlite::Connection,
    key: &Key,
    job: JobId,
) -> Result<Vec<ApprovalRecord>> {
    let mut stmt = conn.prepare(&format!(
        "{SELECT} WHERE job_id = ?1 ORDER BY created_at, id"
    ))?;
    let rows = stmt.query_map([job.to_string()], map)?;
    rows.map(|r| build(r?, key)).collect()
}

fn verify_for_gate(
    conn: &rusqlite::Connection,
    key: &Key,
    job: JobId,
    gate: GateKind,
) -> Result<()> {
    let mut stmt = conn.prepare(&format!("{SELECT} WHERE job_id = ?1 AND gate_kind = ?2"))?;
    let rows = stmt.query_map(params![job.to_string(), to_s(&gate)?], map)?;
    for row in rows {
        build(row?, key)?;
    }
    Ok(())
}

fn sign_row(conn: &rusqlite::Connection, key: &Key, id: &str) -> Result<()> {
    let raw = conn.query_row(&format!("{SELECT} WHERE id = ?1"), [id], map)?;
    let mac = signature(&raw, key)?;
    conn.execute(
        "UPDATE approvals SET hmac = ?1 WHERE id = ?2",
        params![mac, id],
    )?;
    Ok(())
}

pub(crate) fn sign_legacy_rows(conn: &rusqlite::Connection, key: &Key) -> Result<()> {
    let mut stmt = conn.prepare("SELECT id FROM approvals WHERE hmac IS NULL")?;
    let ids = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    for id in ids {
        sign_row(conn, key, &id)?;
    }
    Ok(())
}

impl Tx<'_> {
    pub(crate) fn resign_approval(&self, id: &str) -> Result<()> {
        sign_row(&self.tx, self.approval_key, id)
    }

    pub fn approval(&self, id: ApprovalId) -> Result<ApprovalRecord> {
        approval(&self.tx, self.approval_key, id)
    }

    /// Requests an approval for `binding`. Idempotent: an identical pending approval is
    /// returned as is (`false`). A pending approval with a different subject is superseded.
    pub fn request_approval(
        &mut self,
        binding: &ApprovalBinding,
    ) -> Result<(ApprovalRecord, bool)> {
        let job = self.load_job(binding.job_id)?;
        let gate = binding.gate_kind;
        let hash = binding.subject_hash();

        if let Some(existing) = latest_with_status(
            &self.tx,
            self.approval_key,
            binding.job_id,
            gate,
            ApprovalStatus::Pending,
        )? {
            if existing.subject_hash == hash {
                return Ok((existing, false));
            }
            self.tx.execute(
                "UPDATE approvals SET status = 'superseded' WHERE id = ?1",
                [existing.id.to_string()],
            )?;
            sign_row(&self.tx, self.approval_key, &existing.id.to_string())?;
            self.approval_event(
                &job,
                "approval.superseded",
                &existing,
                serde_json::json!({}),
            )?;
        }
        let id = ApprovalId::new();
        let now = self.now()?;
        self.tx.execute(
            "INSERT INTO approvals(id, job_id, gate_kind, binding_json, subject_hash, status, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 'pending', ?6)",
            params![id.to_string(), binding.job_id.to_string(), to_s(&gate)?, binding.canonical_json(), hash.to_string(), now],
        )?;
        sign_row(&self.tx, self.approval_key, &id.to_string())?;
        let record = approval(&self.tx, self.approval_key, id)?;
        self.approval_event(&job, "approval.requested", &record, serde_json::json!({}))?;
        Ok((record, true))
    }

    /// Records a human decision. The caller must echo the hash it displayed (AP-1); only users
    /// may decide (AP-2); requesting changes or rejecting needs a comment.
    #[allow(clippy::too_many_arguments)]
    pub fn decide_approval(
        &mut self,
        id: ApprovalId,
        decision: Decision,
        echoed_hash: &Sha256,
        actor: &Actor,
        channel: &str,
        comment: Option<&str>,
    ) -> Result<ApprovalRecord> {
        let Actor::User { id: user } = actor else {
            return Err(StoreError::Forbidden(
                "only users can decide approvals".into(),
            ));
        };
        let current = approval(&self.tx, self.approval_key, id)?;
        if current.status != ApprovalStatus::Pending {
            return Err(StoreError::NotPending {
                id: id.to_string(),
                status: to_s(&current.status)?,
            });
        }
        if current.subject_hash != *echoed_hash {
            return Err(StoreError::HashMismatch);
        }
        let comment = comment.map(str::trim).filter(|c| !c.is_empty());
        if decision != Decision::Approve && comment.is_none() {
            return Err(StoreError::CommentRequired);
        }
        let status = match decision {
            Decision::Approve => "approved",
            Decision::RequestChanges => "changes_requested",
            Decision::Reject => "rejected",
        };
        let now = self.now()?;
        self.tx.execute(
            "UPDATE approvals SET status = ?1, decided_by = ?2, decided_at = ?3, channel = ?4, comment = ?5
             WHERE id = ?6 AND status = 'pending'",
            params![status, user, now, channel, comment, id.to_string()],
        )?;
        sign_row(&self.tx, self.approval_key, &id.to_string())?;
        let record = approval(&self.tx, self.approval_key, id)?;
        let job = self.load_job(record.job_id)?;
        self.approval_event(
            &job,
            "approval.decided",
            &record,
            serde_json::json!({ "decision": status, "channel": channel }),
        )?;
        Ok(record)
    }

    /// Marks pending and approved approvals of the given gates as invalidated.
    pub fn invalidate_approvals(
        &mut self,
        job: JobId,
        gates: &[GateKind],
        reason: &str,
    ) -> Result<u32> {
        let job_rec = self.load_job(job)?;
        let mut count = 0;
        for gate in gates {
            let affected: Vec<ApprovalRecord> = {
                let mut stmt = self.tx.prepare(&format!(
                    "{SELECT} WHERE job_id = ?1 AND gate_kind = ?2 AND status IN ('pending','approved')"
                ))?;
                let rows = stmt.query_map(params![job.to_string(), to_s(gate)?], map)?;
                rows.map(|r| build(r?, self.approval_key))
                    .collect::<Result<_>>()?
            };
            for a in affected {
                self.tx.execute(
                    "UPDATE approvals SET status = 'invalidated', invalidated_reason = ?1 WHERE id = ?2",
                    params![reason, a.id.to_string()],
                )?;
                sign_row(&self.tx, self.approval_key, &a.id.to_string())?;
                self.approval_event(
                    &job_rec,
                    "approval.invalidated",
                    &a,
                    serde_json::json!({ "reason": reason }),
                )?;
                count += 1;
            }
        }
        Ok(count)
    }

    fn approval_event(
        &mut self,
        job: &crate::store::JobRecord,
        kind: &str,
        a: &ApprovalRecord,
        extra: serde_json::Value,
    ) -> Result<()> {
        let mut payload = serde_json::json!({
            "approval": a.id.to_string(),
            "gate": to_s(&a.gate)?,
            "subject_hash": a.subject_hash.to_string(),
        });
        if let (Some(p), Some(e)) = (payload.as_object_mut(), extra.as_object()) {
            p.extend(e.clone());
        }
        let actor = if kind == "approval.decided" {
            a.decided_by
                .clone()
                .map_or(Actor::System, |id| Actor::User { id })
        } else {
            Actor::System
        };
        self.append_event(&NewEvent {
            project_id: Some(job.project_id),
            job_id: Some(job.job.id),
            kind: kind.into(),
            payload,
            actor,
        })?;
        Ok(())
    }
}

impl crate::store::Store {
    fn report_integrity_failure<T>(&mut self, result: Result<T>) -> Result<T> {
        if let Err(StoreError::ApprovalIntegrity { id, job }) = &result {
            let id = id.clone();
            let linked = job.parse::<JobId>().ok().and_then(|job_id| {
                self.load_job(job_id)
                    .ok()
                    .map(|record| (job_id, record.project_id))
            });
            let seen: bool = self.raw_connection().query_row(
                "SELECT EXISTS(SELECT 1 FROM events WHERE type='security.approval_integrity_failed' AND json_extract(payload, '$.approval')=?1)",
                [id.as_str()], |r| r.get(0)
            )?;
            if seen {
                return result;
            }
            self.transaction(|tx| {
                tx.append_event(&crate::store::NewEvent {
                    project_id: linked.map(|(_, project_id)| project_id),
                    job_id: linked.map(|(job_id, _)| job_id),
                    kind: "security.approval_integrity_failed".into(),
                    payload: serde_json::json!({"approval": id}),
                    actor: Actor::System,
                })?;
                Ok(())
            })?;
        }
        result
    }

    pub fn approval(&mut self, id: ApprovalId) -> Result<ApprovalRecord> {
        let result = approval(self.raw_connection(), &self.approval_key, id);
        self.report_integrity_failure(result)
    }

    pub fn latest_approved(
        &mut self,
        job: JobId,
        gate: GateKind,
    ) -> Result<Option<ApprovalRecord>> {
        let result = verify_for_gate(self.raw_connection(), &self.approval_key, job, gate)
            .and_then(|()| {
                latest_with_status(
                    self.raw_connection(),
                    &self.approval_key,
                    job,
                    gate,
                    ApprovalStatus::Approved,
                )
            });
        self.report_integrity_failure(result)
    }

    pub fn pending_approval(&self, job: JobId, gate: GateKind) -> Result<Option<ApprovalRecord>> {
        latest_with_status(
            self.raw_connection(),
            &self.approval_key,
            job,
            gate,
            ApprovalStatus::Pending,
        )
    }

    pub fn pending_approvals(&self) -> Result<Vec<ApprovalRecord>> {
        pending(self.raw_connection(), &self.approval_key)
    }

    pub fn list_approvals(&self, job: JobId) -> Result<Vec<ApprovalRecord>> {
        list_for_job(self.raw_connection(), &self.approval_key, job)
    }
}
