//! Artifact revisions and hash-bound approvals at the store level.

use std::sync::Arc;

use turtle_domain::{
    Actor, ApprovalStatus, ArtifactKind, GateKind, JobType, Priority, RunId, Sha256,
};
use turtle_policy::{ApprovalBinding, ArtifactRef};
use turtle_store::{
    ChainVerdict, Decision, JobRecord, NewJob, NewProject, NewRevision, StepClock, Store,
    StoreError,
};
use turtle_workflow::{DecisionContext, JobCommand as C, PreflightDecision, Profile, decide};

fn setup() -> (Store, JobRecord) {
    let mut store = Store::open_in_memory(Arc::new(StepClock::default())).expect("open");
    let rec = store
        .transaction(|tx| {
            let project_id = tx.insert_project(&NewProject {
                slug: "proj".into(),
                key_prefix: "API".into(),
                name: "P".into(),
                description: String::new(),
                repo_path: "/tmp/p".into(),
                default_branch: "main".into(),
                default_profile: "standard".into(),
            })?;
            tx.create_job(
                &NewJob {
                    project_id,
                    title: "t".into(),
                    description: String::new(),
                    job_type: JobType::Feature,
                    priority: Priority::Normal,
                    profile: Profile::standard(),
                },
                &Actor::local_user(),
            )
        })
        .expect("seed");
    (store, rec)
}

fn sha(b: u8) -> Sha256 {
    Sha256::from_bytes([b; 32])
}

fn revision(job: &JobRecord, kind: ArtifactKind, hash: Sha256, author: Actor) -> NewRevision {
    NewRevision {
        job_id: job.job.id,
        kind,
        sha256: hash,
        size_bytes: 10,
        media_type: "application/json".into(),
        schema_id: Some("spec.v1".into()),
        author,
    }
}

fn binding(job: &JobRecord, gate: GateKind, spec_hash: Sha256, rev: u32) -> ApprovalBinding {
    let mut b = ApprovalBinding::new(gate, job.job.id, sha(0xAA), 1);
    b.artifacts.insert(
        "specification".into(),
        ArtifactRef {
            sha256: spec_hash,
            rev,
        },
    );
    b
}

#[test]
fn revisions_are_numbered_linked_and_deduplicated() {
    let (mut s, job) = setup();
    let agent = Actor::AgentRun { id: RunId::new() };
    let r1 = s
        .transaction(|tx| {
            tx.add_revision(&revision(
                &job,
                ArtifactKind::Specification,
                sha(1),
                agent.clone(),
            ))
        })
        .expect("r1");
    assert_eq!((r1.rev, r1.author_kind.as_str()), (1, "agent_run"));

    // Same content again (a replayed step): no new revision, no new event.
    let events_before = s.list_events(None).expect("events").len();
    let again = s
        .transaction(|tx| {
            tx.add_revision(&revision(
                &job,
                ArtifactKind::Specification,
                sha(1),
                agent.clone(),
            ))
        })
        .expect("again");
    assert_eq!(again.id, r1.id);
    assert_eq!(s.list_events(None).expect("events").len(), events_before);

    let r2 = s
        .transaction(|tx| {
            tx.add_revision(&revision(
                &job,
                ArtifactKind::Specification,
                sha(2),
                Actor::local_user(),
            ))
        })
        .expect("r2");
    assert_eq!(
        (r2.rev, r2.author_kind.as_str(), r2.author_id.as_deref()),
        (2, "user", Some("local"))
    );
    assert_eq!(
        s.latest_revision(job.job.id, ArtifactKind::Specification)
            .expect("latest")
            .expect("some")
            .sha256,
        sha(2)
    );
    assert_eq!(
        s.list_revisions(job.job.id, ArtifactKind::Specification)
            .expect("list")
            .len(),
        2
    );

    // Different kinds are independent artifacts.
    let plan = s
        .transaction(|tx| tx.add_revision(&revision(&job, ArtifactKind::Plan, sha(1), agent)))
        .expect("plan");
    assert_eq!(plan.rev, 1);
    assert_eq!(
        s.blob_ref_count(&sha(1)).expect("count"),
        Some(2),
        "same blob referenced by two revisions"
    );
    assert_eq!(s.blob_ref_count(&sha(2)).expect("count"), Some(1));
    assert!(matches!(
        s.verify_chain().expect("verify"),
        ChainVerdict::Intact { .. }
    ));
}

#[test]
fn approval_request_is_idempotent_and_supersedes_changed_subjects() {
    let (mut s, job) = setup();
    let b1 = binding(&job, GateKind::Scope, sha(1), 1);
    let (a1, created) = s
        .transaction(|tx| tx.request_approval(&b1))
        .expect("request");
    assert!(created);
    assert_eq!(a1.status, ApprovalStatus::Pending);
    assert_eq!(a1.subject_hash, b1.subject_hash());
    assert_eq!(a1.binding, b1);

    let (same, created) = s.transaction(|tx| tx.request_approval(&b1)).expect("again");
    assert!(!created);
    assert_eq!(
        same.id, a1.id,
        "replaying the request does not create a second approval"
    );

    let b2 = binding(&job, GateKind::Scope, sha(2), 2);
    let (a2, created) = s
        .transaction(|tx| tx.request_approval(&b2))
        .expect("changed");
    assert!(created);
    assert_eq!(
        s.approval(a1.id).expect("old").status,
        ApprovalStatus::Superseded
    );
    assert_eq!(
        s.pending_approval(job.job.id, GateKind::Scope)
            .expect("pending")
            .expect("some")
            .id,
        a2.id
    );
    assert_eq!(s.pending_approvals().expect("all").len(), 1);
}

#[test]
fn deciding_requires_the_displayed_hash_a_user_and_a_comment_where_needed() {
    let (mut s, job) = setup();
    let b = binding(&job, GateKind::Scope, sha(1), 1);
    let (a, _) = s
        .transaction(|tx| tx.request_approval(&b))
        .expect("request");
    let user = Actor::local_user();

    // Wrong hash: the approver looked at something else.
    let err = s
        .transaction(|tx| {
            tx.decide_approval(a.id, Decision::Approve, &sha(9), &user, "cli_tty", None)
        })
        .expect_err("stale view");
    assert!(matches!(err, StoreError::HashMismatch), "{err}");
    assert_eq!(s.approval(a.id).expect("a").status, ApprovalStatus::Pending);

    // Machines and agents can never approve.
    for who in [
        Actor::System,
        Actor::Scheduler,
        Actor::AgentRun { id: RunId::new() },
    ] {
        let err = s
            .transaction(|tx| {
                tx.decide_approval(
                    a.id,
                    Decision::Approve,
                    &a.subject_hash,
                    &who,
                    "cli_tty",
                    None,
                )
            })
            .expect_err("not a user");
        assert!(matches!(err, StoreError::Forbidden(_)), "{err}");
    }

    // Requesting changes and rejecting need a comment.
    for d in [Decision::RequestChanges, Decision::Reject] {
        for c in [None, Some("   ")] {
            let err = s
                .transaction(|tx| tx.decide_approval(a.id, d, &a.subject_hash, &user, "cli_tty", c))
                .expect_err("comment required");
            assert!(matches!(err, StoreError::CommentRequired));
        }
    }

    let done = s
        .transaction(|tx| {
            tx.decide_approval(
                a.id,
                Decision::Approve,
                &a.subject_hash,
                &user,
                "dashboard",
                None,
            )
        })
        .expect("approve");
    assert_eq!(done.status, ApprovalStatus::Approved);
    assert_eq!(
        (done.decided_by.as_deref(), done.channel.as_deref()),
        (Some("local"), Some("dashboard"))
    );
    assert_eq!(
        s.latest_approved(job.job.id, GateKind::Scope)
            .expect("q")
            .expect("some")
            .id,
        a.id
    );

    // A decision cannot be repeated or changed.
    let err = s
        .transaction(|tx| {
            tx.decide_approval(
                a.id,
                Decision::Reject,
                &a.subject_hash,
                &user,
                "cli_tty",
                Some("changed my mind"),
            )
        })
        .expect_err("already decided");
    assert!(matches!(err, StoreError::NotPending { .. }), "{err}");
    assert!(matches!(
        s.verify_chain().expect("verify"),
        ChainVerdict::Intact { .. }
    ));
}

#[test]
fn explicit_invalidation_covers_pending_and_approved_only() {
    let (mut s, job) = setup();
    let (a, _) = s
        .transaction(|tx| tx.request_approval(&binding(&job, GateKind::Scope, sha(1), 1)))
        .expect("request");
    s.transaction(|tx| {
        tx.decide_approval(
            a.id,
            Decision::Approve,
            &a.subject_hash,
            &Actor::local_user(),
            "cli_tty",
            None,
        )
    })
    .expect("approve");
    let (p, _) = s
        .transaction(|tx| tx.request_approval(&binding(&job, GateKind::Plan, sha(1), 1)))
        .expect("plan approval");

    let n = s
        .transaction(|tx| {
            tx.invalidate_approvals(
                job.job.id,
                &[GateKind::Scope, GateKind::Plan],
                "spec_revised",
            )
        })
        .expect("invalidate");
    assert_eq!(n, 2);
    assert_eq!(
        s.approval(a.id).expect("a").status,
        ApprovalStatus::Invalidated
    );
    assert_eq!(
        s.approval(p.id).expect("p").invalidated_reason.as_deref(),
        Some("spec_revised")
    );
    assert!(
        s.latest_approved(job.job.id, GateKind::Scope)
            .expect("q")
            .is_none()
    );
    let again = s
        .transaction(|tx| tx.invalidate_approvals(job.job.id, &[GateKind::Scope], "spec_revised"))
        .expect("again");
    assert_eq!(
        again, 0,
        "already invalidated approvals are not touched twice"
    );
}

#[test]
fn a_material_change_decided_by_the_engine_invalidates_approvals_in_the_same_transaction() {
    let (mut s, mut job) = setup();
    let mut cx = DecisionContext {
        preflight: Some(PreflightDecision::Allow),
        dependencies_delivered: true,
        flags_acknowledged: true,
        ..DecisionContext::default()
    };
    let _ = &mut cx;
    let user = Actor::local_user();
    let step = |s: &mut Store, job: &JobRecord, cmd: C, actor: &Actor| -> JobRecord {
        let d = decide(&job.job, &cmd, actor, &cx).expect("accepted");
        s.transaction(|tx| {
            tx.apply_decision(job.job.version, &d, &cmd, actor)?;
            tx.load_job(job.job.id)
        })
        .expect("persist")
    };
    job = step(&mut s, &job, C::Start, &user);
    job = step(
        &mut s,
        &job,
        C::SpecProduced {
            valid: true,
            criteria_count: 2,
        },
        &Actor::Scheduler,
    );
    assert_eq!(format!("{:?}", job.job.state), "ScopeApproval");

    let (a, _) = s
        .transaction(|tx| tx.request_approval(&binding(&job, GateKind::Scope, sha(1), 1)))
        .expect("request");
    // The user edits the spec while it awaits approval: a new SpecProduced in ScopeApproval.
    step(
        &mut s,
        &job,
        C::SpecProduced {
            valid: true,
            criteria_count: 3,
        },
        &user,
    );

    let after = s.approval(a.id).expect("approval");
    assert_eq!(after.status, ApprovalStatus::Invalidated);
    assert_eq!(after.invalidated_reason.as_deref(), Some("spec_revised"));
    let kinds: Vec<_> = s
        .list_events(Some(job.job.id))
        .expect("events")
        .into_iter()
        .map(|e| e.kind)
        .collect();
    assert!(
        kinds.contains(&"approval.invalidated".to_owned()),
        "{kinds:?}"
    );
}
