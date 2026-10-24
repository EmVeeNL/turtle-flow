//! Hash-bound approvals through the real engine and context: an approval can never be used for
//! anything other than what the approver was shown (invariants AP-1, AP-4, J-4, J-5).

use std::sync::{Arc, Mutex};

use tempfile::TempDir;
use turtle_artifacts::BlobStore;
use turtle_domain::{
    AcceptanceCriterion, Actor, ApprovalStatus, ArtifactKind, GateKind, JobId, JobState, JobType,
    Plan, PlanTask, Priority, RunId, Specification, VerificationMethod,
};
use turtle_policy::GitBinding;
use turtle_scheduler::{
    ApprovalError, ApprovalService, ContextConfig, CrashPlan, Engine, EngineError, GitFacts,
    GitFactsProvider, SharedStore, StoreContext,
};
use turtle_store::{
    Decision, JobRecord, ManualClock, NewJob, NewProject, NewRevision, Store, StoreError,
};
use turtle_workflow::{JobCommand as C, Profile, ReviewVerdict, VerificationOutcome};

#[derive(Default)]
struct FakeGit(Mutex<Option<GitFacts>>);

impl FakeGit {
    fn set(&self, head: &str, flags: &[&str]) {
        *self.0.lock().expect("lock") = Some(GitFacts {
            binding: GitBinding {
                base_commit: "base0001".into(),
                head_commit: head.into(),
                diff_sha256: turtle_domain::Sha256::from_bytes([head.len() as u8; 32]),
                target_branch: "main".into(),
            },
            required_flags: flags.iter().map(ToString::to_string).collect(),
        });
    }
}

impl GitFactsProvider for FakeGit {
    async fn facts(&self, _job: &JobRecord) -> Option<GitFacts> {
        self.0.lock().expect("lock").clone()
    }
}

struct Env {
    _dir: TempDir,
    store: SharedStore,
    blobs: Arc<BlobStore>,
    git: Arc<FakeGit>,
    engine: Engine<StoreContext<FakeGit>>,
    service: ApprovalService<StoreContext<FakeGit>, FakeGit>,
    job: JobId,
}

async fn env(profile: Profile) -> Env {
    let dir = tempfile::tempdir().expect("tempdir");
    let blobs = Arc::new(BlobStore::open(dir.path().join("blobs")).expect("blobs"));
    let store =
        SharedStore::new(Store::open_in_memory(Arc::new(ManualClock::new())).expect("store"));
    let git = Arc::new(FakeGit::default());
    let ctx = Arc::new(StoreContext::new(
        store.clone(),
        Arc::clone(&blobs),
        Arc::clone(&git),
        ContextConfig::default(),
    ));
    let engine = Engine::new(store.clone(), ctx, CrashPlan::never());
    let service = ApprovalService::new(engine.clone(), Arc::clone(&git));
    let job = store
        .with(move |s| {
            s.transaction(|tx| {
                let project_id = tx.insert_project(&NewProject {
                    slug: "proj".into(),
                    key_prefix: "API".into(),
                    name: "P".into(),
                    description: String::new(),
                    repo_path: "/tmp/p".into(),
                    default_branch: "main".into(),
                    default_profile: "standard".into(),
                })?;
                Ok(tx
                    .create_job(
                        &NewJob {
                            project_id,
                            title: "t".into(),
                            description: String::new(),
                            job_type: JobType::Feature,
                            priority: Priority::Normal,
                            profile,
                        },
                        &Actor::local_user(),
                    )?
                    .job
                    .id)
            })
        })
        .await
        .expect("job");
    Env {
        _dir: dir,
        store,
        blobs,
        git,
        engine,
        service,
        job,
    }
}

fn spec(extra_criteria: usize) -> Specification {
    let mut criteria = vec![AcceptanceCriterion {
        id: "AC-1".into(),
        statement: "Over-limit requests get 429".into(),
        verification: VerificationMethod::Test,
    }];
    for i in 0..extra_criteria {
        criteria.push(AcceptanceCriterion {
            id: format!("AC-{}", i + 2),
            statement: "more".into(),
            verification: VerificationMethod::Review,
        });
    }
    Specification {
        summary: "Add rate limiting".into(),
        in_scope: vec![],
        out_of_scope: vec![],
        criteria,
        affected_areas: vec!["src/http/**".into()],
        risks: vec![],
        assumptions: vec![],
    }
}

fn plan(tasks: usize) -> Plan {
    Plan {
        tasks: (1..=tasks)
            .map(|i| PlanTask {
                id: format!("T{i}"),
                title: format!("task {i}"),
                instructions: "do it".into(),
                expected_paths: vec![],
                criteria: if i == 1 { vec!["AC-1".into()] } else { vec![] },
                depends_on: vec![],
                complexity: None,
            })
            .collect(),
    }
}

impl Env {
    async fn put<T: serde::Serialize>(&self, kind: ArtifactKind, doc: &T, author: Actor) {
        let hash = self.blobs.put_json(doc).expect("blob");
        let size = turtle_artifacts::canonical_json(doc).expect("json").len() as u64;
        let job = self.job;
        self.store
            .with(move |s| {
                s.transaction(|tx| {
                    tx.add_revision(&NewRevision {
                        job_id: job,
                        kind,
                        sha256: hash,
                        size_bytes: size,
                        media_type: "application/json".into(),
                        schema_id: None,
                        author,
                    })
                })
            })
            .await
            .expect("revision");
    }

    async fn submit(&self, cmd: C, actor: Actor) -> Result<JobRecord, EngineError> {
        self.engine.submit(self.job, cmd, actor).await
    }

    async fn state(&self) -> JobState {
        let job = self.job;
        self.store
            .with(move |s| s.load_job(job))
            .await
            .expect("load")
            .job
            .state
    }

    async fn pending(&self, gate: GateKind) -> turtle_store::ApprovalRecord {
        let job = self.job;
        self.store
            .with(move |s| s.pending_approval(job, gate))
            .await
            .expect("query")
            .expect("a pending approval")
    }

    /// Drives the job to `scope_approval` with a stored specification.
    async fn to_scope_approval(&self, extra: usize) {
        self.submit(C::Start, Actor::local_user())
            .await
            .expect("start");
        let agent = Actor::AgentRun { id: RunId::new() };
        self.put(ArtifactKind::Specification, &spec(extra), agent)
            .await;
        self.submit(
            C::SpecProduced {
                valid: true,
                criteria_count: 1 + u32::try_from(extra).expect("small"),
            },
            Actor::Scheduler,
        )
        .await
        .expect("spec produced");
        assert_eq!(self.state().await, JobState::ScopeApproval);
    }

    async fn approve(&self, gate: GateKind, ack: Vec<String>) -> Result<JobRecord, ApprovalError> {
        let a = self.pending(gate).await;
        self.service
            .decide(
                a.id,
                Decision::Approve,
                a.subject_hash,
                Actor::local_user(),
                "cli_tty",
                None,
                ack,
            )
            .await
    }
}

#[tokio::test]
async fn scope_approval_moves_the_job_only_after_a_valid_human_decision() {
    let e = env(Profile::standard()).await;
    e.to_scope_approval(1).await;

    // Without an approval, the guard refuses (J-4).
    let err = e
        .submit(
            C::ApprovalGranted {
                gate: GateKind::Scope,
            },
            Actor::local_user(),
        )
        .await
        .expect_err("no approval yet");
    assert!(
        matches!(&err, EngineError::Rejected(r) if r.guard == "approval_valid"),
        "{err}"
    );

    let (a, created) = e
        .service
        .request(e.job, GateKind::Scope)
        .await
        .expect("request");
    assert!(created);
    assert_eq!(a.status, ApprovalStatus::Pending);
    assert_eq!(
        e.state().await,
        JobState::ScopeApproval,
        "requesting does not approve"
    );

    let job = e.approve(GateKind::Scope, vec![]).await.expect("approve");
    assert_eq!(job.job.state, JobState::Planning);
}

#[tokio::test]
async fn approving_with_a_different_hash_than_the_one_shown_is_refused() {
    let e = env(Profile::standard()).await;
    e.to_scope_approval(1).await;
    let (a, _) = e
        .service
        .request(e.job, GateKind::Scope)
        .await
        .expect("request");

    let wrong = turtle_domain::Sha256::from_bytes([1; 32]);
    let err = e
        .service
        .decide(
            a.id,
            Decision::Approve,
            wrong,
            Actor::local_user(),
            "cli_tty",
            None,
            vec![],
        )
        .await
        .expect_err("hash mismatch");
    assert!(
        matches!(err, ApprovalError::Store(StoreError::HashMismatch)),
        "{err}"
    );
    assert_eq!(e.state().await, JobState::ScopeApproval);
}

#[tokio::test]
async fn a_spec_that_changed_after_the_request_cannot_be_approved() {
    let e = env(Profile::standard()).await;
    e.to_scope_approval(1).await;
    let (a, _) = e
        .service
        .request(e.job, GateKind::Scope)
        .await
        .expect("request");

    // A new revision appears (say, a user edit) before the approver clicks.
    e.put(ArtifactKind::Specification, &spec(2), Actor::local_user())
        .await;
    let err = e
        .service
        .decide(
            a.id,
            Decision::Approve,
            a.subject_hash,
            Actor::local_user(),
            "cli_tty",
            None,
            vec![],
        )
        .await
        .expect_err("stale view");
    assert!(matches!(err, ApprovalError::Stale), "{err}");
    let job = e.job;
    let status = e
        .store
        .with(move |s| s.approval(a.id))
        .await
        .expect("approval")
        .status;
    assert_eq!(status, ApprovalStatus::Invalidated);
    assert_eq!(e.state().await, JobState::ScopeApproval);

    // Requesting again binds to the new revision and works.
    let (b, created) = e
        .service
        .request(job, GateKind::Scope)
        .await
        .expect("re-request");
    assert!(created && b.subject_hash != a.subject_hash);
    assert_eq!(
        e.approve(GateKind::Scope, vec![])
            .await
            .expect("approve")
            .job
            .state,
        JobState::Planning
    );
}

#[tokio::test]
async fn editing_the_spec_through_the_workflow_invalidates_the_pending_approval() {
    let e = env(Profile::standard()).await;
    e.to_scope_approval(1).await;
    let (a, _) = e
        .service
        .request(e.job, GateKind::Scope)
        .await
        .expect("request");

    e.put(ArtifactKind::Specification, &spec(2), Actor::local_user())
        .await;
    e.submit(
        C::SpecProduced {
            valid: true,
            criteria_count: 3,
        },
        Actor::local_user(),
    )
    .await
    .expect("edit");

    let err = e
        .service
        .decide(
            a.id,
            Decision::Approve,
            a.subject_hash,
            Actor::local_user(),
            "cli_tty",
            None,
            vec![],
        )
        .await
        .expect_err("already invalidated");
    assert!(
        matches!(err, ApprovalError::Store(StoreError::NotPending { .. })),
        "{err}"
    );
}

#[tokio::test]
async fn an_approval_recorded_earlier_cannot_satisfy_the_guard_after_the_spec_changes() {
    let e = env(Profile::standard()).await;
    e.to_scope_approval(1).await;
    let (a, _) = e
        .service
        .request(e.job, GateKind::Scope)
        .await
        .expect("request");
    // Record the decision directly (as if the engine step after it never happened, e.g. a crash).
    e.store
        .with(move |s| {
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
        })
        .await
        .expect("decide");

    // The spec changes before the engine applies the decision: the guard recomputes and refuses.
    e.put(ArtifactKind::Specification, &spec(5), Actor::local_user())
        .await;
    let err = e
        .submit(
            C::ApprovalGranted {
                gate: GateKind::Scope,
            },
            Actor::local_user(),
        )
        .await
        .expect_err("stale approval");
    assert!(
        matches!(&err, EngineError::Rejected(r) if r.guard == "approval_valid"),
        "{err}"
    );
    assert_eq!(e.state().await, JobState::ScopeApproval);
}

#[tokio::test]
async fn only_users_decide_and_changes_or_rejections_need_a_comment() {
    let e = env(Profile::standard()).await;
    e.to_scope_approval(1).await;
    let (a, _) = e
        .service
        .request(e.job, GateKind::Scope)
        .await
        .expect("request");

    for who in [
        Actor::AgentRun { id: RunId::new() },
        Actor::System,
        Actor::Scheduler,
    ] {
        let err = e
            .service
            .decide(
                a.id,
                Decision::Approve,
                a.subject_hash,
                who,
                "cli_tty",
                None,
                vec![],
            )
            .await
            .expect_err("not a user");
        assert!(
            matches!(err, ApprovalError::Store(StoreError::Forbidden(_))),
            "{err}"
        );
    }
    let err = e
        .service
        .decide(
            a.id,
            Decision::RequestChanges,
            a.subject_hash,
            Actor::local_user(),
            "cli_tty",
            None,
            vec![],
        )
        .await
        .expect_err("comment required");
    assert!(matches!(
        err,
        ApprovalError::Store(StoreError::CommentRequired)
    ));
    assert_eq!(e.state().await, JobState::ScopeApproval);

    let job = e
        .service
        .decide(
            a.id,
            Decision::RequestChanges,
            a.subject_hash,
            Actor::local_user(),
            "cli_tty",
            Some("narrow the scope".into()),
            vec![],
        )
        .await
        .expect("request changes");
    assert_eq!(job.job.state, JobState::Specification);
}

#[tokio::test]
async fn rejecting_cancels_the_job() {
    let e = env(Profile::standard()).await;
    e.to_scope_approval(1).await;
    let (a, _) = e
        .service
        .request(e.job, GateKind::Scope)
        .await
        .expect("request");
    let job = e
        .service
        .decide(
            a.id,
            Decision::Reject,
            a.subject_hash,
            Actor::local_user(),
            "dashboard",
            Some("not wanted".into()),
            vec![],
        )
        .await
        .expect("reject");
    assert_eq!(job.job.state, JobState::Cancelled);
}

#[tokio::test]
async fn large_plans_need_a_plan_approval_even_in_standard() {
    let e = env(Profile::standard()).await;
    e.to_scope_approval(1).await;
    e.service
        .request(e.job, GateKind::Scope)
        .await
        .expect("request");
    e.approve(GateKind::Scope, vec![])
        .await
        .expect("approve scope");

    e.put(ArtifactKind::Plan, &plan(9), Actor::Scheduler).await;
    let job = e
        .submit(
            C::PlanProduced {
                valid: true,
                within_scope: true,
            },
            Actor::Scheduler,
        )
        .await
        .expect("plan");
    assert_eq!(
        job.job.state,
        JobState::PlanApproval,
        "9 tasks exceed the threshold of 8"
    );
}

/// Drives a standard job to `acceptance_approval` with real artifacts.
async fn to_acceptance(e: &Env) {
    e.to_scope_approval(1).await;
    e.service
        .request(e.job, GateKind::Scope)
        .await
        .expect("request");
    e.approve(GateKind::Scope, vec![]).await.expect("scope");
    e.put(ArtifactKind::Plan, &plan(2), Actor::Scheduler).await;
    e.submit(
        C::PlanProduced {
            valid: true,
            within_scope: true,
        },
        Actor::Scheduler,
    )
    .await
    .expect("plan");
    for cmd in [
        C::AllTasksSucceeded,
        C::GatesFinished {
            all_required_passed: true,
        },
        C::ReviewCompleted {
            verdict: ReviewVerdict::Pass,
        },
    ] {
        e.submit(cmd, Actor::Scheduler).await.expect("progress");
    }
    e.put(
        ArtifactKind::VerificationReport,
        &serde_json::json!({"met": ["AC-1"]}),
        Actor::Scheduler,
    )
    .await;
    e.submit(
        C::VerificationCompleted {
            outcome: VerificationOutcome::Met,
        },
        Actor::Scheduler,
    )
    .await
    .expect("verified");
    assert_eq!(e.state().await, JobState::AcceptanceApproval);
}

#[tokio::test]
async fn acceptance_binds_to_the_commit_and_requires_acknowledging_flags() {
    let e = env(Profile::standard()).await;
    to_acceptance(&e).await;

    // No Git state yet: acceptance cannot even be requested.
    let err = e
        .service
        .request(e.job, GateKind::Acceptance)
        .await
        .expect_err("no git facts");
    assert!(matches!(err, ApprovalError::Binding(_)), "{err}");

    e.git.set(
        "head-1",
        &["dependency:governor", "protected_path:Cargo.lock"],
    );
    let (a, _) = e
        .service
        .request(e.job, GateKind::Acceptance)
        .await
        .expect("request");
    assert_eq!(
        a.binding.acknowledged_flags,
        ["dependency:governor", "protected_path:Cargo.lock"]
    );

    let err = e
        .approve(GateKind::Acceptance, vec!["dependency:governor".into()])
        .await
        .expect_err("one flag missing");
    assert!(
        matches!(&err, ApprovalError::FlagsNotAcknowledged(m) if m == &["protected_path:Cargo.lock"]),
        "{err}"
    );
    assert_eq!(e.state().await, JobState::AcceptanceApproval);

    let job = e
        .approve(
            GateKind::Acceptance,
            vec![
                "dependency:governor".into(),
                "protected_path:Cargo.lock".into(),
            ],
        )
        .await
        .expect("approve with all flags");
    assert_eq!(job.job.state, JobState::Delivery);
}

#[tokio::test]
async fn a_new_commit_after_the_request_makes_the_view_stale() {
    let e = env(Profile::standard()).await;
    to_acceptance(&e).await;
    e.git.set("head-1", &[]);
    let (a, _) = e
        .service
        .request(e.job, GateKind::Acceptance)
        .await
        .expect("request");

    e.git.set("head-22", &[]);
    let err = e
        .service
        .decide(
            a.id,
            Decision::Approve,
            a.subject_hash,
            Actor::local_user(),
            "cli_tty",
            None,
            vec![],
        )
        .await
        .expect_err("code changed");
    assert!(matches!(err, ApprovalError::Stale), "{err}");
    assert_eq!(e.state().await, JobState::AcceptanceApproval);
}

#[tokio::test]
async fn a_commit_after_the_decision_blocks_delivery() {
    let e = env(Profile::standard()).await;
    to_acceptance(&e).await;
    e.git.set("head-1", &[]);
    let (a, _) = e
        .service
        .request(e.job, GateKind::Acceptance)
        .await
        .expect("request");
    e.store
        .with(move |s| {
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
        })
        .await
        .expect("decide");

    // Code changes between the human's click and the transition (J-5).
    e.git.set("head-22", &[]);
    let err = e
        .submit(
            C::ApprovalGranted {
                gate: GateKind::Acceptance,
            },
            Actor::local_user(),
        )
        .await
        .expect_err("diff changed");
    assert!(
        matches!(&err, EngineError::Rejected(r) if r.guard == "approval_valid"),
        "{err}"
    );
    assert_eq!(
        e.state().await,
        JobState::AcceptanceApproval,
        "delivery must not start"
    );

    // Restoring the exact approved state makes the same approval valid again.
    e.git.set("head-1", &[]);
    assert_eq!(
        e.submit(
            C::ApprovalGranted {
                gate: GateKind::Acceptance
            },
            Actor::local_user()
        )
        .await
        .expect("valid again")
        .job
        .state,
        JobState::Delivery
    );
}
