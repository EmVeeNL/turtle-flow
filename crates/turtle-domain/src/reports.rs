//! Review and verification reports, plus the deterministic rules that turn them into workflow
//! outcomes. An agent writes the findings; the factory decides what they mean.

use serde::{Deserialize, Serialize};

use crate::documents::{Specification, VerificationMethod};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Nit,
    Minor,
    Major,
    Blocker,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Finding {
    pub id: String,
    pub severity: Severity,
    pub category: String,
    #[serde(default)]
    pub file: Option<String>,
    #[serde(default)]
    pub line: Option<u32>,
    pub message: String,
    #[serde(default)]
    pub suggestion: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewReport {
    pub summary: String,
    #[serde(default)]
    pub findings: Vec<Finding>,
    /// The reviewer judges the approach wrong; the plan needs rework.
    #[serde(default)]
    pub replan: bool,
    /// The change goes beyond the approved scope.
    #[serde(default)]
    pub scope_creep: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewOutcome {
    Pass,
    /// Major or blocking findings: send back to implementation.
    Fixable,
    Replan,
    ScopeCreep,
}

impl ReviewReport {
    /// Scope creep outranks a replan, which outranks fixable findings. Minor findings and nits
    /// are recorded and shown to the human but do not block.
    #[must_use]
    pub fn outcome(&self) -> ReviewOutcome {
        if self.scope_creep {
            ReviewOutcome::ScopeCreep
        } else if self.replan {
            ReviewOutcome::Replan
        } else if self.findings.iter().any(|f| f.severity >= Severity::Major) {
            ReviewOutcome::Fixable
        } else {
            ReviewOutcome::Pass
        }
    }

    /// Findings that must be fixed before the review can pass.
    #[must_use]
    pub fn blocking_findings(&self) -> Vec<&Finding> {
        self.findings
            .iter()
            .filter(|f| f.severity >= Severity::Major)
            .collect()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CriterionStatus {
    Met,
    NotMet,
    Unverifiable,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CriterionResult {
    pub id: String,
    pub status: CriterionStatus,
    #[serde(default)]
    pub evidence: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerificationReport {
    pub criteria: Vec<CriterionResult>,
    /// Unmet criteria need a new plan, not just a fix.
    #[serde(default)]
    pub structural: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationDecision {
    Met,
    NotMetFixable,
    NotMetStructural,
}

impl VerificationReport {
    /// Every criterion of the specification must be accounted for. A missing criterion counts as
    /// not met. `unverifiable` is acceptable only for criteria judged by review or by a human;
    /// for tests and commands it means the check did not happen.
    #[must_use]
    pub fn decision(&self, spec: &Specification) -> VerificationDecision {
        let all_ok = spec.criteria.iter().all(|c| {
            match self
                .criteria
                .iter()
                .find(|r| r.id == c.id)
                .map(|r| r.status)
            {
                Some(CriterionStatus::Met) => true,
                Some(CriterionStatus::Unverifiable) => !c.verification.needs_task(),
                Some(CriterionStatus::NotMet) | None => false,
            }
        });
        match (all_ok, self.structural) {
            (true, _) => VerificationDecision::Met,
            (false, true) => VerificationDecision::NotMetStructural,
            (false, false) => VerificationDecision::NotMetFixable,
        }
    }

    /// Criteria the human must judge themselves at acceptance.
    #[must_use]
    pub fn manual_criteria<'a>(&self, spec: &'a Specification) -> Vec<&'a str> {
        spec.criteria
            .iter()
            .filter(|c| {
                c.verification == VerificationMethod::Manual
                    || self
                        .criteria
                        .iter()
                        .any(|r| r.id == c.id && r.status == CriterionStatus::Unverifiable)
            })
            .map(|c| c.id.as_str())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::documents::AcceptanceCriterion;

    fn finding(sev: Severity) -> Finding {
        Finding {
            id: "R-1".into(),
            severity: sev,
            category: "correctness".into(),
            file: None,
            line: None,
            message: "m".into(),
            suggestion: None,
        }
    }

    fn spec() -> Specification {
        let c = |id: &str, v| AcceptanceCriterion {
            id: id.into(),
            statement: "s".into(),
            verification: v,
        };
        Specification {
            summary: "s".into(),
            in_scope: vec![],
            out_of_scope: vec![],
            criteria: vec![
                c("AC-1", VerificationMethod::Test),
                c("AC-2", VerificationMethod::Review),
            ],
            affected_areas: vec![],
            risks: vec![],
            assumptions: vec![],
        }
    }

    fn result(id: &str, status: CriterionStatus) -> CriterionResult {
        CriterionResult {
            id: id.into(),
            status,
            evidence: String::new(),
        }
    }

    #[test]
    fn review_outcome_priorities() {
        let mut r = ReviewReport::default();
        assert_eq!(r.outcome(), ReviewOutcome::Pass);
        r.findings = vec![finding(Severity::Minor), finding(Severity::Nit)];
        assert_eq!(
            r.outcome(),
            ReviewOutcome::Pass,
            "minor findings do not block"
        );
        r.findings.push(finding(Severity::Major));
        assert_eq!(r.outcome(), ReviewOutcome::Fixable);
        assert_eq!(r.blocking_findings().len(), 1);
        r.replan = true;
        assert_eq!(r.outcome(), ReviewOutcome::Replan);
        r.scope_creep = true;
        assert_eq!(r.outcome(), ReviewOutcome::ScopeCreep);
    }

    #[test]
    fn a_blocker_can_never_pass() {
        let r = ReviewReport {
            summary: "looks fine".into(),
            findings: vec![finding(Severity::Blocker)],
            ..ReviewReport::default()
        };
        assert_eq!(r.outcome(), ReviewOutcome::Fixable);
    }

    #[test]
    fn verification_decisions() {
        let s = spec();
        let all_met = VerificationReport {
            criteria: vec![
                result("AC-1", CriterionStatus::Met),
                result("AC-2", CriterionStatus::Met),
            ],
            structural: false,
        };
        assert_eq!(all_met.decision(&s), VerificationDecision::Met);

        let missing = VerificationReport {
            criteria: vec![result("AC-1", CriterionStatus::Met)],
            structural: false,
        };
        assert_eq!(
            missing.decision(&s),
            VerificationDecision::NotMetFixable,
            "a missing criterion is not met"
        );

        let unverifiable_test = VerificationReport {
            criteria: vec![
                result("AC-1", CriterionStatus::Unverifiable),
                result("AC-2", CriterionStatus::Met),
            ],
            structural: false,
        };
        assert_eq!(
            unverifiable_test.decision(&s),
            VerificationDecision::NotMetFixable,
            "a test that did not run is not met"
        );

        let unverifiable_review = VerificationReport {
            criteria: vec![
                result("AC-1", CriterionStatus::Met),
                result("AC-2", CriterionStatus::Unverifiable),
            ],
            structural: false,
        };
        assert_eq!(unverifiable_review.decision(&s), VerificationDecision::Met);
        assert_eq!(unverifiable_review.manual_criteria(&s), ["AC-2"]);

        let structural = VerificationReport {
            criteria: vec![
                result("AC-1", CriterionStatus::NotMet),
                result("AC-2", CriterionStatus::Met),
            ],
            structural: true,
        };
        assert_eq!(
            structural.decision(&s),
            VerificationDecision::NotMetStructural
        );
    }

    #[test]
    fn unknown_fields_are_rejected() {
        assert!(
            serde_json::from_str::<ReviewReport>(r#"{"summary":"x","approved":true}"#).is_err()
        );
        assert!(
            serde_json::from_str::<VerificationReport>(r#"{"criteria":[],"override":"met"}"#)
                .is_err()
        );
    }
}
