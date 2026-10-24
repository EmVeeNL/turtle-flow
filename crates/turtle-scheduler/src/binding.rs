//! Builds approval binding documents from the current state of a job (`docs/WORKFLOW.md` §6.2).
//!
//! The same function is used when an approval is *requested* and whenever a guard *checks* one,
//! so an approval is valid exactly when rebuilding the binding from the world as it is now gives
//! the hash the approver saw.

use std::future::Future;

use sha2::{Digest, Sha256 as Hasher};
use turtle_domain::{GateKind, Sha256};
use turtle_policy::{ApprovalBinding, ArtifactRef, GitBinding};
use turtle_store::{JobRecord, RevisionRecord};
use turtle_workflow::Profile;

/// Version of the security policy (protected paths, gates). Bumped when policy changes in a way
/// that should invalidate acceptance approvals.
pub const POLICY_VERSION: u32 = 1;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum BindingError {
    #[error("the {0} is not available yet, so this approval cannot be requested")]
    Missing(&'static str),
    #[error("{0:?} approvals have no binding builder")]
    Unsupported(GateKind),
}

/// Latest revisions of the artifacts approvals can bind to.
#[derive(Debug, Clone, Default)]
pub struct ArtifactSnapshot {
    pub specification: Option<RevisionRecord>,
    pub plan: Option<RevisionRecord>,
    pub review_report: Option<RevisionRecord>,
    pub verification_report: Option<RevisionRecord>,
}

/// Git state an acceptance approval binds to, plus the flags a human must acknowledge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitFacts {
    pub binding: GitBinding,
    /// Sorted flags such as `protected_path:Cargo.lock` or `dependency:governor`.
    pub required_flags: Vec<String>,
}

/// Supplies current Git facts for a job (head commit, diff hash, flags).
pub trait GitFactsProvider: Send + Sync + 'static {
    fn facts(&self, job: &JobRecord) -> impl Future<Output = Option<GitFacts>> + Send;
}

/// For jobs and tests without a worktree: acceptance approvals cannot be built.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoGit;

impl GitFactsProvider for NoGit {
    async fn facts(&self, _job: &JobRecord) -> Option<GitFacts> {
        None
    }
}

#[must_use]
pub fn profile_hash(profile: &Profile) -> Sha256 {
    let json = serde_json::to_value(profile)
        .and_then(|v| serde_json::to_string(&v))
        .expect("profiles always serialize");
    Sha256::from_bytes(Hasher::digest(json.as_bytes()).into())
}

fn artifact_ref(rev: &RevisionRecord) -> ArtifactRef {
    ArtifactRef {
        sha256: rev.sha256,
        rev: rev.rev,
    }
}

fn require<'a>(
    rev: Option<&'a RevisionRecord>,
    what: &'static str,
) -> Result<&'a RevisionRecord, BindingError> {
    rev.ok_or(BindingError::Missing(what))
}

/// Builds the binding for `gate` from the artifacts and Git facts as they are now.
pub fn build_binding(
    job: &JobRecord,
    gate: GateKind,
    artifacts: &ArtifactSnapshot,
    git: Option<&GitFacts>,
    policy_version: u32,
) -> Result<ApprovalBinding, BindingError> {
    let mut b = ApprovalBinding::new(
        gate,
        job.job.id,
        profile_hash(&job.job.profile),
        policy_version,
    );
    match gate {
        GateKind::Scope => {
            let spec = require(artifacts.specification.as_ref(), "specification")?;
            b.artifacts
                .insert("specification".into(), artifact_ref(spec));
        }
        GateKind::Plan => {
            let spec = require(artifacts.specification.as_ref(), "specification")?;
            let plan = require(artifacts.plan.as_ref(), "plan")?;
            b.artifacts
                .insert("specification".into(), artifact_ref(spec));
            b.artifacts.insert("plan".into(), artifact_ref(plan));
        }
        GateKind::Acceptance => {
            let spec = require(artifacts.specification.as_ref(), "specification")?;
            let plan = require(artifacts.plan.as_ref(), "plan")?;
            let verification = require(
                artifacts.verification_report.as_ref(),
                "verification report",
            )?;
            let git = git.ok_or(BindingError::Missing("Git state of the job"))?;
            b.artifacts
                .insert("specification".into(), artifact_ref(spec));
            b.artifacts.insert("plan".into(), artifact_ref(plan));
            b.artifacts
                .insert("verification_report".into(), artifact_ref(verification));
            if let Some(review) = &artifacts.review_report {
                b.artifacts
                    .insert("review_report".into(), artifact_ref(review));
            }
            b.git = Some(git.binding.clone());
            b.acknowledged_flags = git.required_flags.clone();
            b.acknowledged_flags.sort();
            b.acknowledged_flags.dedup();
        }
        other => return Err(BindingError::Unsupported(other)),
    }
    Ok(b)
}
