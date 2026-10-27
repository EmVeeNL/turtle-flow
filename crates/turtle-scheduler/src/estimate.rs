//! Stable per-plan token envelopes used by approval policy.

use serde::Serialize;
use turtle_domain::{Complexity, Plan, Role, Specification};
use turtle_store::{JobRecord, StoreError};
use turtle_tokens::{BucketPath, Confidence, cold_start_estimate, estimate, score_complexity};

use crate::shared::SharedStore;

#[derive(Clone, Debug, Serialize)]
pub struct PlanEstimate {
    pub p10: u64,
    pub p50: u64,
    pub p90: u64,
    pub envelope_tokens: u64,
    pub confidence: &'static str,
}

/// Estimate the same sequential role mix as `turtle budget estimate` and freeze that result
/// beside the plan artifact. Approval checks then use a stable estimate even as history grows.
pub async fn for_job(
    store: &SharedStore,
    job: &JobRecord,
    spec: &Specification,
    plan: &Plan,
) -> Result<PlanEstimate, StoreError> {
    let mut roles = vec![Role::SpecWriter, Role::Planner];
    if job.job.profile.discovery_enabled(job.job.complexity) {
        roles.insert(0, Role::Discovery);
    }
    roles.extend(std::iter::repeat_n(Role::Implementer, plan.tasks.len()));
    roles.extend([Role::Reviewer, Role::Verifier]);

    let project = job.project_id.to_string();
    let spec = spec.clone();
    let plan = plan.clone();
    let estimates = store
        .with(move |db| {
            roles
                .into_iter()
                .map(|role| {
                    let complexity = match role {
                        Role::SpecWriter => Complexity::XS,
                        Role::Implementer | Role::Fixer => {
                            score_complexity(&spec, Some(&plan)).class
                        }
                        _ => score_complexity(&spec, None).class,
                    };
                    let path = BucketPath::new(&project, role, complexity, "", "");
                    let buckets = db.estimation_buckets(&path)?;
                    let broad = [None, None, None, buckets[3].clone()];
                    Ok(estimate(&broad, cold_start_estimate(role)))
                })
                .collect::<Result<Vec<_>, StoreError>>()
        })
        .await?;

    let confidence = estimates
        .iter()
        .map(|item| item.confidence)
        .min_by_key(|confidence| match confidence {
            Confidence::Unknown => 0,
            Confidence::Low => 1,
            Confidence::Medium => 2,
            Confidence::High => 3,
        })
        .unwrap_or(Confidence::Unknown);
    let p10 = estimates
        .iter()
        .fold(0_u64, |sum, item| sum.saturating_add(item.p10));
    let p50 = estimates
        .iter()
        .fold(0_u64, |sum, item| sum.saturating_add(item.p50));
    let p90 = estimates
        .iter()
        .fold(0_u64, |sum, item| sum.saturating_add(item.p90));
    let envelope_tokens = ((p90 as f64) * (1.0 + confidence.buffer()))
        .ceil()
        .min(u64::MAX as f64) as u64;
    Ok(PlanEstimate {
        p10,
        p50,
        p90,
        envelope_tokens,
        confidence: confidence.label(),
    })
}
