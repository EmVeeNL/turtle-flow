//! The real [`ContextProvider`]: computes the facts `decide()` needs from the store, artifacts
//! and Git. The important one is approval validity, which is **recomputed** from current state
//! every time (invariant AP-4): an approval counts only if rebuilding its binding now gives the
//! hash that was approved.

use std::sync::Arc;

use turtle_artifacts::BlobStore;
use turtle_domain::{ApprovalValidity, ArtifactKind, Complexity, GateKind, Plan};
use turtle_store::JobRecord;
use turtle_workflow::{DecisionContext, PreflightDecision, ScopeApprovalMode};

use crate::binding::{ArtifactSnapshot, GitFactsProvider, POLICY_VERSION, build_binding};
use crate::engine::ContextProvider;
use crate::shared::SharedStore;

#[derive(Debug, Clone)]
pub struct ContextConfig {
    /// Placeholder until the token engine's preflight exists (milestone M3): always this value.
    pub preflight: PreflightDecision,
    /// Automatic scope approval in `fast` jobs. **Off by default**: it needs protected-path
    /// checks that do not exist yet, and auto-approval is risk-bearing.
    pub auto_scope_enabled: bool,
    /// Plans with more tasks than this need a plan approval even in `standard`.
    pub plan_task_threshold: usize,
    pub policy_version: u32,
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            preflight: PreflightDecision::Allow,
            auto_scope_enabled: false,
            plan_task_threshold: 8,
            policy_version: POLICY_VERSION,
        }
    }
}

pub struct StoreContext<G> {
    store: SharedStore,
    blobs: Arc<BlobStore>,
    git: Arc<G>,
    config: ContextConfig,
}

impl<G> std::fmt::Debug for StoreContext<G> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StoreContext").finish_non_exhaustive()
    }
}

impl<G: GitFactsProvider> StoreContext<G> {
    #[must_use]
    pub fn new(
        store: SharedStore,
        blobs: Arc<BlobStore>,
        git: Arc<G>,
        config: ContextConfig,
    ) -> Self {
        Self {
            store,
            blobs,
            git,
            config,
        }
    }
}

/// Everything read from the store in one blocking call.
pub(crate) struct Snapshot {
    pub artifacts: ArtifactSnapshot,
    pub approved: Vec<(GateKind, turtle_store::ApprovalRecord)>,
    pub dependencies_delivered: bool,
}

pub(crate) async fn read_snapshot(
    store: &SharedStore,
    job: &JobRecord,
) -> Result<Snapshot, turtle_store::StoreError> {
    let id = job.job.id;
    store
        .with(move |s| {
            let latest = |k| s.latest_revision(id, k);
            let artifacts = ArtifactSnapshot {
                specification: latest(ArtifactKind::Specification)?,
                plan: latest(ArtifactKind::Plan)?,
                review_report: latest(ArtifactKind::ReviewReport)?,
                verification_report: latest(ArtifactKind::VerificationReport)?,
            };
            let mut approved = Vec::new();
            for gate in [GateKind::Scope, GateKind::Plan, GateKind::Acceptance] {
                if let Some(a) = s.latest_approved(id, gate)? {
                    approved.push((gate, a));
                }
            }
            Ok(Snapshot {
                artifacts,
                approved,
                dependencies_delivered: s.dependencies_delivered(id)?,
            })
        })
        .await
}

impl<G: GitFactsProvider> ContextProvider for StoreContext<G> {
    async fn context(&self, job: &JobRecord) -> DecisionContext {
        // Fail closed: if the facts cannot be read, nothing is approved and nothing proceeds.
        let Ok(snap) = read_snapshot(&self.store, job).await else {
            return DecisionContext::default();
        };
        let git = self.git.facts(job).await;

        let mut cx = DecisionContext {
            preflight: Some(self.config.preflight),
            dependencies_delivered: snap.dependencies_delivered,
            ..DecisionContext::default()
        };

        for gate in [GateKind::Scope, GateKind::Plan, GateKind::Acceptance] {
            let validity = match snap.approved.iter().find(|(g, _)| *g == gate) {
                None => ApprovalValidity::Missing,
                Some((_, approved)) => {
                    // Valid only if the world still hashes to what the human approved.
                    match build_binding(
                        job,
                        gate,
                        &snap.artifacts,
                        git.as_ref(),
                        self.config.policy_version,
                    ) {
                        Ok(expected) if expected.subject_hash() == approved.subject_hash => {
                            ApprovalValidity::Valid
                        }
                        _ => ApprovalValidity::Invalid,
                    }
                }
            };
            cx.approvals.insert(gate, validity);
        }
        // Flags are part of the binding and acknowledgement is enforced when the approval is
        // decided, so a valid acceptance approval implies they were acknowledged.
        cx.flags_acknowledged = cx.approval(GateKind::Acceptance) == ApprovalValidity::Valid;

        cx.plan_thresholds_exceeded = match &snap.artifacts.plan {
            None => false,
            Some(rev) => self
                .blobs
                .get(&rev.sha256)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<Plan>(&bytes).ok())
                .is_none_or(|plan| plan.task_count() > self.config.plan_task_threshold),
        };

        cx.scope_auto_approvable = self.config.auto_scope_enabled
            && job.job.profile.scope_approval == ScopeApprovalMode::AutoIfSmall
            && job.job.complexity.is_some_and(|c| c <= Complexity::S)
            && self.config.preflight == PreflightDecision::Allow;
        cx
    }
}
