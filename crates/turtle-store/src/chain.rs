//! Hash chain over the event log (SEC-009, `docs/SECURITY.md` §6.7).
//!
//! `hash = SHA-256(prev_hash_hex ‖ canonical_json(event without hash))`. The first event's
//! `prev_hash` is the genesis hash derived from the installation id. Canonical JSON means object
//! keys sorted and no insignificant whitespace; the `seq` is deliberately not hashed because it
//! is assigned by the database after the hash is computed.

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

/// Event fields covered by the hash.
#[derive(Debug, Clone)]
pub struct HashedEvent<'a> {
    pub id: &'a str,
    pub occurred_at: &'a str,
    pub project_id: Option<&'a str>,
    pub job_id: Option<&'a str>,
    pub task_id: Option<&'a str>,
    pub run_id: Option<&'a str>,
    pub kind: &'a str,
    pub payload: &'a Value,
    pub actor_kind: &'a str,
    pub actor_id: &'a str,
}

#[must_use]
pub fn genesis_hash(installation_id: &str) -> String {
    hex(Sha256::digest(
        format!("turtle-flow:genesis:{installation_id}").as_bytes(),
    ))
}

#[must_use]
pub fn event_hash(prev_hash: &str, e: &HashedEvent<'_>) -> String {
    // `json!` builds a `Value`, whose objects are sorted maps, so the output key order is stable.
    let doc = json!({
        "id": e.id,
        "occurred_at": e.occurred_at,
        "project_id": e.project_id,
        "job_id": e.job_id,
        "task_id": e.task_id,
        "run_id": e.run_id,
        "type": e.kind,
        "payload": e.payload,
        "actor_kind": e.actor_kind,
        "actor_id": e.actor_id,
    });
    let canonical = serde_json::to_string(&doc).expect("event document is always serializable");
    hex(Sha256::digest(format!("{prev_hash}{canonical}").as_bytes()))
}

pub(crate) fn hex(bytes: impl AsRef<[u8]>) -> String {
    bytes.as_ref().iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample<'a>(payload: &'a Value, kind: &'a str) -> HashedEvent<'a> {
        HashedEvent {
            id: "01JB7Y0Q4N3T9Z8K2M6W5R1C7D",
            occurred_at: "2026-10-24T00:00:00.000Z",
            project_id: None,
            job_id: Some("01JB7Y0Q4N3T9Z8K2M6W5R1C7E"),
            task_id: None,
            run_id: None,
            kind,
            payload,
            actor_kind: "user",
            actor_id: "local",
        }
    }

    #[test]
    fn hash_depends_on_previous_hash_and_every_field() {
        let payload = json!({"b": 1, "a": 2});
        let g = genesis_hash("inst");
        let base = event_hash(&g, &sample(&payload, "job.stage_entered"));
        assert_eq!(base.len(), 64);
        assert_eq!(base, event_hash(&g, &sample(&payload, "job.stage_entered")));
        assert_ne!(
            base,
            event_hash("x", &sample(&payload, "job.stage_entered"))
        );
        assert_ne!(base, event_hash(&g, &sample(&payload, "job.blocked")));
        let other = json!({"b": 1, "a": 3});
        assert_ne!(base, event_hash(&g, &sample(&other, "job.stage_entered")));
    }

    #[test]
    fn payload_key_order_does_not_matter() {
        let a: Value = serde_json::from_str(r#"{"x":1,"y":{"p":1,"q":2}}"#).expect("json");
        let b: Value = serde_json::from_str(r#"{"y":{"q":2,"p":1},"x":1}"#).expect("json");
        let g = genesis_hash("inst");
        assert_eq!(
            event_hash(&g, &sample(&a, "k")),
            event_hash(&g, &sample(&b, "k"))
        );
    }

    #[test]
    fn genesis_differs_per_installation() {
        assert_ne!(genesis_hash("a"), genesis_hash("b"));
    }
}
