//! Versioned provider capability snapshots: round trip, dedupe of identical consecutive
//! snapshots, and the snapshot a run records when it starts.

use std::sync::Arc;

use serde_json::json;
use turtle_domain::{Actor, JobType, Priority, Role, RunId};
use turtle_store::{
    ChainVerdict, JobRecord, NewJob, NewProject, NewRun, StepClock, Store, capability_hash,
    read_capability_snapshots,
};
use turtle_workflow::Profile;

fn store() -> Store {
    Store::open_in_memory(Arc::new(StepClock::default())).expect("open")
}

fn seed(store: &mut Store) -> JobRecord {
    store
        .transaction(|tx| {
            let project_id = tx.insert_project(&NewProject {
                slug: "payments-api".into(),
                key_prefix: "API".into(),
                name: "Payments API".into(),
                description: String::new(),
                repo_path: "/tmp/payments-api".into(),
                default_branch: "main".into(),
                default_profile: "standard".into(),
            })?;
            tx.create_job(
                &NewJob {
                    project_id,
                    title: "Add rate limiting".into(),
                    description: "details".into(),
                    job_type: JobType::Feature,
                    priority: Priority::High,
                    profile: Profile::standard(),
                },
                &Actor::local_user(),
            )
        })
        .expect("seed")
}

fn begin(store: &mut Store, job: &JobRecord, provider: &str) -> RunId {
    let run = RunId::new();
    store
        .transaction(|tx| {
            tx.begin_run(&NewRun {
                id: run,
                job_id: job.job.id,
                role: Role::Implementer,
                stage: "implement".into(),
                provider: provider.into(),
                model: "default".into(),
                adapter_version: "test".into(),
                context_manifest_rev: None,
            })
        })
        .expect("run");
    run
}

#[test]
fn snapshots_round_trip_and_identical_consecutive_ones_are_deduped() {
    let mut store = store();
    let caps = json!({"effort_levels": ["low", "max"], "native_json_schema": true});
    let (first, new) = store
        .transaction(|tx| tx.record_capability_snapshot("claude-code", Some("2.1.310"), &caps))
        .expect("first");
    assert!(new);
    assert_eq!(first.version, 1);
    assert_eq!(first.cli_version.as_deref(), Some("2.1.310"));
    assert_eq!(
        first.content_hash,
        capability_hash("claude-code", Some("2.1.310"), &caps)
    );

    let (again, new) = store
        .transaction(|tx| tx.record_capability_snapshot("claude-code", Some("2.1.310"), &caps))
        .expect("again");
    assert!(!new, "identical snapshot is not stored twice");
    assert_eq!(again, first);

    let (upgraded, new) = store
        .transaction(|tx| tx.record_capability_snapshot("claude-code", Some("2.1.311"), &caps))
        .expect("upgrade");
    assert!(new, "a CLI version change is a new snapshot");
    assert_eq!(upgraded.version, 2);
    assert_ne!(upgraded.content_hash, first.content_hash);

    // Going back to the first content is a change from the latest, so it is stored again.
    let (back, new) = store
        .transaction(|tx| tx.record_capability_snapshot("claude-code", Some("2.1.310"), &caps))
        .expect("downgrade");
    assert!(new);
    assert_eq!(back.version, 3);
    assert_eq!(back.content_hash, first.content_hash);

    let history = store
        .capability_snapshots("claude-code", 10)
        .expect("history");
    assert_eq!(
        history.iter().map(|s| s.version).collect::<Vec<_>>(),
        [3, 2, 1]
    );
    assert_eq!(history[2], first);
    assert!(
        store
            .capability_snapshots("codex", 10)
            .expect("other")
            .is_empty()
    );
}

#[test]
fn a_run_records_the_snapshot_it_started_with_and_keeps_the_chain_valid() {
    let mut store = store();
    let job = seed(&mut store);
    let unrecorded = begin(&mut store, &job, "codex");
    let caps = json!({"sandbox_modes": ["read-only"]});
    let (v1, _) = store
        .transaction(|tx| tx.record_capability_snapshot("codex", Some("0.163.0"), &caps))
        .expect("v1");
    let first = begin(&mut store, &job, "codex");
    let (v2, _) = store
        .transaction(|tx| tx.record_capability_snapshot("codex", Some("0.163.1"), &caps))
        .expect("v2");
    let second = begin(&mut store, &job, "codex");

    assert_eq!(
        store.run_capability_snapshot(unrecorded).expect("read"),
        None
    );
    assert_eq!(
        store.run_capability_snapshot(first).expect("read"),
        Some(v1.clone())
    );
    assert_eq!(
        store.run_capability_snapshot(second).expect("read"),
        Some(v2.clone())
    );

    let runs = store.runs_for_job(job.job.id).expect("runs");
    let by_id = |id: RunId| {
        runs.iter()
            .find(|r| r.id == id.to_string())
            .expect("run listed")
    };
    assert_eq!(by_id(unrecorded).capability_snapshot_hash, None);
    assert_eq!(by_id(first).cli_version.as_deref(), Some("0.163.0"));
    assert_eq!(
        by_id(first).capability_snapshot_hash.as_deref(),
        Some(v1.content_hash.as_str())
    );
    assert_eq!(
        by_id(second).capability_snapshot_hash.as_deref(),
        Some(v2.content_hash.as_str())
    );
    assert!(matches!(
        store.verify_chain().expect("verify"),
        ChainVerdict::Intact { .. }
    ));
}

#[test]
fn doctor_reads_snapshots_without_creating_a_database() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = dir.path().join("turtle.db");
    assert_eq!(
        read_capability_snapshots(&db, "codex", 2).expect("absent"),
        None
    );
    assert!(!db.exists(), "reading never creates the database");

    let mut store = Store::open(&db, Arc::new(StepClock::default())).expect("open");
    store
        .transaction(|tx| tx.record_capability_snapshot("codex", Some("0.163.0"), &json!({"a": 1})))
        .expect("record");
    drop(store);
    let read = read_capability_snapshots(&db, "codex", 2)
        .expect("read")
        .expect("schema has snapshots");
    assert_eq!(read.len(), 1);
    assert_eq!(read[0].cli_version.as_deref(), Some("0.163.0"));
}
