//! Pure token estimation from structured documents and reported run history.

use turtle_domain::{Complexity, Plan, Role, Specification};

const Z90: f64 = 1.281_551_565_544_600_4;
const SHRINK: f64 = 10.0;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Factor {
    pub name: &'static str,
    pub count: usize,
    pub points: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComplexityScore {
    pub class: Complexity,
    pub score: u8,
    pub factors: Vec<Factor>,
}

/// Scores only features represented in today's specification and plan.
#[must_use]
pub fn score_complexity(spec: &Specification, plan: Option<&Plan>) -> ComplexityScore {
    let mut factors = Vec::new();
    let description = spec.summary.len() + spec.in_scope.iter().map(String::len).sum::<usize>();
    factors.push(Factor {
        name: "description",
        count: description,
        points: scale(description, 600, 5),
    });
    factors.push(Factor {
        name: "criteria",
        count: spec.criteria.len(),
        points: scale(spec.criteria.len(), 8, 15),
    });
    factors.push(Factor {
        name: "affected_areas",
        count: spec.affected_areas.len(),
        points: scale(spec.affected_areas.len(), 12, 20),
    });
    let risks = spec
        .risks
        .iter()
        .filter(|r| {
            [
                "security",
                "migration",
                "concurrency",
                "public_api",
                "data_loss",
            ]
            .iter()
            .any(|tag| r.to_ascii_lowercase().contains(tag))
        })
        .count();
    factors.push(Factor {
        name: "risk_tags",
        count: risks,
        points: scale(risks, 2, 20),
    });
    if let Some(plan) = plan {
        let depth = dependency_depth(plan);
        factors.push(Factor {
            name: "plan_tasks",
            count: plan.tasks.len(),
            points: scale(plan.tasks.len(), 8, 7),
        });
        factors.push(Factor {
            name: "dependency_depth",
            count: depth,
            points: scale(depth, 4, 3),
        });
    }
    let raw = factors.iter().map(|f| u16::from(f.points)).sum::<u16>();
    // The omitted design inputs have no data source yet. Normalize the available weights so
    // that the full XS..XL range is still reachable without inventing file or language facts.
    let available = if plan.is_some() { 70 } else { 60 };
    let score = ((raw * 100) / available).min(100) as u8;
    ComplexityScore {
        class: Complexity::from_score(score),
        score,
        factors,
    }
}

fn scale(value: usize, full: usize, weight: u8) -> u8 {
    ((value.min(full) * usize::from(weight)) / full) as u8
}

fn dependency_depth(plan: &Plan) -> usize {
    let Ok(order) = plan.execution_order() else {
        return 0;
    };
    let mut depths = std::collections::BTreeMap::new();
    for task in order {
        let depth = task
            .depends_on
            .iter()
            .filter_map(|id| depths.get(id))
            .copied()
            .max()
            .unwrap_or(0)
            + 1;
        depths.insert(&task.id, depth);
    }
    depths.values().copied().max().unwrap_or(0)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BucketPath(pub [String; 4]);

impl BucketPath {
    #[must_use]
    pub fn new(
        project: &str,
        role: Role,
        complexity: Complexity,
        provider: &str,
        model: &str,
    ) -> Self {
        let base = format!("r:{}|c:{complexity:?}", role_name(role));
        let provider = format!("{base}|pv:{}", escape(provider));
        let model = format!("{provider}|m:{}", escape(model));
        Self([
            format!("p:{}|{model}", escape(project)),
            model,
            provider,
            base,
        ])
    }
}

fn escape(value: &str) -> String {
    let mut out = String::new();
    for b in value.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn role_name(role: Role) -> &'static str {
    match role {
        Role::Discovery => "discovery",
        Role::SpecWriter => "spec_writer",
        Role::Planner => "planner",
        Role::Implementer => "implementer",
        Role::Fixer => "fixer",
        Role::Reviewer => "reviewer",
        Role::Verifier => "verifier",
        Role::Summarizer => "summarizer",
        Role::ReleaseNotes => "release_notes",
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct BucketStats {
    pub n: u64,
    pub mean_ln: f64,
    pub m2_ln: f64,
    pub reservoir: Vec<u64>,
}

impl Default for BucketStats {
    fn default() -> Self {
        Self {
            n: 0,
            mean_ln: 0.0,
            m2_ln: 0.0,
            reservoir: Vec::new(),
        }
    }
}

impl BucketStats {
    pub fn observe(&mut self, tokens: u64) {
        if tokens == 0 {
            return;
        }
        self.n += 1;
        let x = (tokens as f64).ln();
        let delta = x - self.mean_ln;
        self.mean_ln += delta / self.n as f64;
        self.m2_ln += delta * (x - self.mean_ln);
        if self.reservoir.len() < 200 {
            self.reservoir.push(tokens);
        } else {
            let mut x = self.n.wrapping_add(0x9E37_79B9_7F4A_7C15);
            x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            let slot = ((x ^ (x >> 31)) % self.n) as usize;
            if slot < 200 {
                self.reservoir[slot] = tokens;
            }
        }
    }

    fn variance(&self) -> f64 {
        if self.n > 1 {
            self.m2_ln / (self.n - 1) as f64
        } else {
            0.0
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Confidence {
    Unknown,
    Low,
    Medium,
    High,
}

impl Confidence {
    #[must_use]
    pub const fn buffer(self) -> f64 {
        match self {
            Self::High => 0.15,
            Self::Medium => 0.25,
            Self::Low | Self::Unknown => 0.50,
        }
    }
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EstimateBasis {
    ColdStart,
    Bucket { level: usize, observations: u64 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Estimate {
    pub p10: u64,
    pub p50: u64,
    pub p90: u64,
    pub confidence: Confidence,
    pub basis: EstimateBasis,
}

/// Buckets are ordered most specific first; absent levels are `None`.
#[must_use]
pub fn estimate(buckets: &[Option<BucketStats>; 4], cold_start: u64) -> Estimate {
    let Some(level) = buckets
        .iter()
        .position(|b| b.as_ref().is_some_and(|b| b.n > 0))
    else {
        return Estimate {
            p10: (cold_start / 4).max(1),
            p50: cold_start,
            p90: cold_start.saturating_mul(4),
            confidence: Confidence::Unknown,
            basis: EstimateBasis::ColdStart,
        };
    };
    let mut mu = (cold_start.max(1) as f64).ln();
    let mut variance = 0.7;
    for bucket in buckets[level..].iter().rev().flatten().filter(|b| b.n > 0) {
        let n = bucket.n as f64;
        mu = (n * bucket.mean_ln + SHRINK * mu) / (n + SHRINK);
        variance = ((n * bucket.variance() + SHRINK * variance) / (n + SHRINK)).max(0.05);
    }
    let bucket = buckets[level].as_ref().expect("selected populated bucket");
    let cv = (variance.exp() - 1.0).sqrt();
    let confidence = if bucket.n >= 30 && cv <= 0.6 {
        Confidence::High
    } else if bucket.n >= 10 {
        Confidence::Medium
    } else {
        Confidence::Low
    };
    let sigma = variance.sqrt();
    let mut p10 = quantile(mu - Z90 * sigma);
    let p50 = quantile(mu);
    let mut p90 = quantile(mu + Z90 * sigma);
    if bucket.n >= 30 && bucket.reservoir.len() >= 30 {
        let mut sample = bucket.reservoir.clone();
        sample.sort_unstable();
        let empirical10 = sample[sample.len() / 10];
        let empirical90 = sample[sample.len() * 9 / 10];
        if ratio_diff(empirical10, p10) > 0.25 {
            p10 = empirical10;
        }
        if ratio_diff(empirical90, p90) > 0.25 {
            p90 = empirical90;
        }
    }
    Estimate {
        p10: p10.min(p50),
        p50,
        p90: p90.max(p50),
        confidence,
        basis: EstimateBasis::Bucket {
            level,
            observations: bucket.n,
        },
    }
}

fn ratio_diff(a: u64, b: u64) -> f64 {
    a.abs_diff(b) as f64 / b.max(1) as f64
}
fn quantile(x: f64) -> u64 {
    if x.exp().is_finite() {
        x.exp().ceil().clamp(1.0, u64::MAX as f64) as u64
    } else {
        u64::MAX
    }
}

#[must_use]
pub const fn cold_start_estimate(role: Role) -> u64 {
    match role {
        Role::Discovery | Role::Planner | Role::Verifier => 25_000,
        Role::SpecWriter => 20_000,
        Role::Implementer | Role::Fixer => 40_000,
        Role::Reviewer => 30_000,
        Role::Summarizer | Role::ReleaseNotes => 10_000,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReservationSize {
    pub tokens: u64,
    pub estimate: Estimate,
}

#[must_use]
pub fn reservation_size(estimate: Estimate) -> ReservationSize {
    let tokens = if estimate.basis == EstimateBasis::ColdStart {
        estimate.p50
    } else {
        ((estimate.p90 as f64) * (1.0 + estimate.confidence.buffer()))
            .ceil()
            .min(u64::MAX as f64) as u64
    };
    ReservationSize { tokens, estimate }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Warn,
    RequireApproval,
    Deny,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BudgetInput {
    pub hard_headroom: Option<u64>,
    pub project_remaining: Option<u64>,
    pub soft_headroom: Option<u64>,
    pub max_without_approval: Option<u64>,
    pub provider_available: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Preflight {
    pub complexity: ComplexityScore,
    pub estimate: Estimate,
    pub envelope_tokens: u64,
    pub decision: Decision,
    pub reasons: Vec<&'static str>,
}

/// Sums projected sequential runs. Retry and fix-loop rates are unavailable today.
#[must_use]
pub fn preflight(
    spec: &Specification,
    plan: &Plan,
    runs: &[(Role, Estimate)],
    budget: &BudgetInput,
) -> Preflight {
    let complexity = score_complexity(spec, Some(plan));
    let confidence = runs
        .iter()
        .map(|(_, e)| e.confidence)
        .min_by_key(|c| match c {
            Confidence::Unknown => 0,
            Confidence::Low => 1,
            Confidence::Medium => 2,
            Confidence::High => 3,
        })
        .unwrap_or(Confidence::Unknown);
    let estimate = Estimate {
        p10: runs.iter().fold(0u64, |n, (_, e)| n.saturating_add(e.p10)),
        p50: runs.iter().fold(0u64, |n, (_, e)| n.saturating_add(e.p50)),
        p90: runs.iter().fold(0u64, |n, (_, e)| n.saturating_add(e.p90)),
        confidence,
        basis: EstimateBasis::ColdStart,
    };
    let envelope_tokens = ((estimate.p90 as f64) * (1.0 + confidence.buffer()))
        .ceil()
        .min(u64::MAX as f64) as u64;
    let (decision, reason) = if !budget.provider_available {
        (Decision::Deny, "no eligible provider")
    } else if budget.hard_headroom.is_some_and(|h| estimate.p50 > h) {
        (Decision::Deny, "median exceeds hard budget")
    } else if complexity.class == Complexity::XL {
        (
            Decision::RequireApproval,
            "XL complexity; consider splitting",
        )
    } else if budget.hard_headroom.is_some_and(|h| envelope_tokens > h) {
        (Decision::RequireApproval, "envelope exceeds hard budget")
    } else if matches!(confidence, Confidence::Unknown | Confidence::Low)
        && budget
            .project_remaining
            .is_some_and(|h| estimate.p90 > h / 2)
    {
        (
            Decision::RequireApproval,
            "low confidence and large project share",
        )
    } else if budget
        .max_without_approval
        .is_some_and(|h| envelope_tokens > h)
    {
        (
            Decision::RequireApproval,
            "profile approval threshold exceeded",
        )
    } else if budget.soft_headroom.is_some_and(|h| envelope_tokens > h) {
        (Decision::Warn, "soft budget may be exceeded")
    } else {
        (Decision::Allow, "estimate fits applicable budgets")
    };
    Preflight {
        complexity,
        estimate,
        envelope_tokens,
        decision,
        reasons: vec![reason],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use turtle_domain::{AcceptanceCriterion, PlanTask, VerificationMethod};

    fn documents(large: bool) -> (Specification, Plan) {
        let count = if large { 12 } else { 1 };
        (
            Specification {
                summary: "feature".repeat(if large { 100 } else { 1 }),
                in_scope: vec![],
                out_of_scope: vec![],
                criteria: (0..count)
                    .map(|i| AcceptanceCriterion {
                        id: format!("AC-{i}"),
                        statement: "works".into(),
                        verification: VerificationMethod::Test,
                    })
                    .collect(),
                affected_areas: (0..count).map(|i| format!("src/{i}")).collect(),
                risks: if large {
                    vec!["security".into(), "migration".into()]
                } else {
                    vec![]
                },
                assumptions: vec![],
            },
            Plan {
                tasks: (0..count)
                    .map(|i| PlanTask {
                        id: format!("T{i}"),
                        title: "task".into(),
                        instructions: "do".into(),
                        expected_paths: vec![],
                        criteria: vec![format!("AC-{i}")],
                        depends_on: if i > 0 {
                            vec![format!("T{}", i - 1)]
                        } else {
                            vec![]
                        },
                        complexity: None,
                    })
                    .collect(),
            },
        )
    }

    #[test]
    fn cold_start_is_wide_and_reserves_the_existing_number() {
        let e = estimate(&[None, None, None, None], 40_000);
        assert_eq!(e.confidence, Confidence::Unknown);
        assert_eq!((e.p10, e.p50, e.p90), (10_000, 40_000, 160_000));
        assert_eq!(reservation_size(e).tokens, 40_000);
    }

    #[test]
    fn fallback_uses_the_first_populated_level() {
        let mut parent = BucketStats::default();
        parent.observe(1_200);
        let e = estimate(&[None, None, Some(parent), None], 40_000);
        assert_eq!(
            e.basis,
            EstimateBasis::Bucket {
                level: 2,
                observations: 1
            }
        );
    }

    #[test]
    fn reservoir_is_bounded_and_replaces_more_than_one_slot() {
        let mut b = BucketStats::default();
        for n in 1..=1_000 {
            b.observe(n);
        }
        assert_eq!(b.reservoir.len(), 200);
        assert!(b.reservoir.iter().filter(|&&n| n > 200).count() > 1);
    }

    #[test]
    fn complexity_and_preflight_follow_decision_order() {
        let (small, plan) = documents(false);
        let (large, large_plan) = documents(true);
        assert_eq!(
            score_complexity(&large, Some(&large_plan)).class,
            Complexity::XL
        );
        let runs = [(
            Role::Implementer,
            Estimate {
                p10: 1_000,
                p50: 2_000,
                p90: 4_000,
                confidence: Confidence::Low,
                basis: EstimateBasis::ColdStart,
            },
        )];
        let mut budget = BudgetInput {
            hard_headroom: Some(10_000),
            project_remaining: None,
            soft_headroom: None,
            max_without_approval: None,
            provider_available: true,
        };
        assert_eq!(
            preflight(&small, &plan, &runs, &budget).decision,
            Decision::Allow
        );
        budget.hard_headroom = Some(1_000);
        assert_eq!(
            preflight(&large, &large_plan, &runs, &budget).decision,
            Decision::Deny
        );
        budget.hard_headroom = Some(10_000);
        assert_eq!(
            preflight(&large, &large_plan, &runs, &budget).decision,
            Decision::RequireApproval
        );
        budget.hard_headroom = Some(3_000);
        assert_eq!(
            preflight(&small, &plan, &runs, &budget).decision,
            Decision::RequireApproval
        );
        budget.hard_headroom = None;
        budget.soft_headroom = Some(1_000);
        assert_eq!(
            preflight(&small, &plan, &runs, &budget).decision,
            Decision::Warn
        );
        budget.provider_available = false;
        assert_eq!(
            preflight(&small, &plan, &runs, &budget).decision,
            Decision::Deny
        );
    }

    proptest! {
        #[test]
        fn estimates_are_ordered(value in 1u64..1_000_000, count in 1usize..100) {
            let mut b = BucketStats::default();
            for _ in 0..count { b.observe(value); }
            let e = estimate(&[Some(b), None, None, None], 40_000);
            prop_assert!(e.p10 > 0 && e.p10 <= e.p50 && e.p50 <= e.p90);
        }
        #[test]
        fn shrinkage_moves_toward_observation(value in 1u64..100_000) {
            let mut b = BucketStats::default(); b.observe(value);
            let one = estimate(&[Some(b.clone()), None, None, None], 40_000).p50;
            for _ in 0..99 { b.observe(value); }
            let many = estimate(&[Some(b), None, None, None], 40_000).p50;
            prop_assert!(many.abs_diff(value) <= one.abs_diff(value) + 1);
        }
        #[test]
        fn confidence_does_not_decrease(value in 1u64..100_000) {
            let mut b = BucketStats::default();
            for _ in 0..10 { b.observe(value); }
            let before = estimate(&[Some(b.clone()), None, None, None], 40_000);
            for _ in 0..20 { b.observe(value); }
            let after = estimate(&[Some(b), None, None, None], 40_000);
            prop_assert_eq!(before.confidence, Confidence::Medium);
            prop_assert!(matches!(after.confidence, Confidence::Medium | Confidence::High));
            prop_assert!(after.p90 - after.p10 <= before.p90 - before.p10,
                "repeated identical observations should narrow the interval");
        }
    }
}
