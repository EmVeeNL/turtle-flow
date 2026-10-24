//! Structured documents produced by agents or edited by users: the specification and the plan
//! (`docs/PRD.md` FR-024, FR-026). Validation here is deterministic; an agent's claim that its
//! output is valid is never trusted.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::roles::Complexity;

/// How an acceptance criterion is verified (FR-024).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationMethod {
    /// A named automated test.
    Test,
    /// A command whose exit status decides.
    Command,
    /// Judged in AI review.
    Review,
    /// Judged by a human.
    Manual,
}

impl VerificationMethod {
    /// Criteria verified by tests or commands must be covered by implementation tasks.
    #[must_use]
    pub const fn needs_task(self) -> bool {
        matches!(self, Self::Test | Self::Command)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptanceCriterion {
    pub id: String,
    pub statement: String,
    pub verification: VerificationMethod,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Specification {
    pub summary: String,
    #[serde(default)]
    pub in_scope: Vec<String>,
    #[serde(default)]
    pub out_of_scope: Vec<String>,
    pub criteria: Vec<AcceptanceCriterion>,
    /// Paths or areas the change is expected to touch; used for scope conformance.
    #[serde(default)]
    pub affected_areas: Vec<String>,
    #[serde(default)]
    pub risks: Vec<String>,
    #[serde(default)]
    pub assumptions: Vec<String>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SpecError {
    #[error("the summary is empty")]
    EmptySummary,
    #[error("the specification has no acceptance criteria")]
    NoCriteria,
    #[error("criterion id `{0}` is empty or contains whitespace")]
    BadCriterionId(String),
    #[error("duplicate criterion id `{0}`")]
    DuplicateCriterion(String),
    #[error("criterion `{0}` has an empty statement")]
    EmptyStatement(String),
}

impl Specification {
    pub fn validate(&self) -> Result<(), SpecError> {
        if self.summary.trim().is_empty() {
            return Err(SpecError::EmptySummary);
        }
        if self.criteria.is_empty() {
            return Err(SpecError::NoCriteria);
        }
        let mut seen = BTreeSet::new();
        for c in &self.criteria {
            if c.id.is_empty() || c.id.chars().any(char::is_whitespace) {
                return Err(SpecError::BadCriterionId(c.id.clone()));
            }
            if !seen.insert(c.id.as_str()) {
                return Err(SpecError::DuplicateCriterion(c.id.clone()));
            }
            if c.statement.trim().is_empty() {
                return Err(SpecError::EmptyStatement(c.id.clone()));
            }
        }
        Ok(())
    }

    #[must_use]
    pub fn criteria_count(&self) -> u32 {
        u32::try_from(self.criteria.len()).unwrap_or(u32::MAX)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanTask {
    pub id: String,
    pub title: String,
    pub instructions: String,
    /// Globs of paths this task is expected to change (conflict prediction, scope checks).
    #[serde(default)]
    pub expected_paths: Vec<String>,
    /// Acceptance criteria this task contributes to.
    #[serde(default)]
    pub criteria: Vec<String>,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub complexity: Option<Complexity>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub tasks: Vec<PlanTask>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PlanError {
    #[error("the plan has no tasks")]
    NoTasks,
    #[error("duplicate task id `{0}`")]
    DuplicateTask(String),
    #[error("task `{task}` depends on unknown task `{dependency}`")]
    UnknownDependency { task: String, dependency: String },
    #[error("task `{0}` depends on itself")]
    SelfDependency(String),
    #[error("the task dependencies contain a cycle involving `{0}`")]
    Cycle(String),
    #[error("task `{task}` references unknown criterion `{criterion}`")]
    UnknownCriterion { task: String, criterion: String },
    #[error("criterion `{0}` is verified by a test or command but no task covers it")]
    UncoveredCriterion(String),
}

impl Plan {
    /// Checks the plan against the specification it implements (FR-026).
    pub fn validate(&self, spec: &Specification) -> Result<(), PlanError> {
        if self.tasks.is_empty() {
            return Err(PlanError::NoTasks);
        }
        let mut ids = BTreeSet::new();
        for t in &self.tasks {
            if !ids.insert(t.id.as_str()) {
                return Err(PlanError::DuplicateTask(t.id.clone()));
            }
        }
        for t in &self.tasks {
            for d in &t.depends_on {
                if *d == t.id {
                    return Err(PlanError::SelfDependency(t.id.clone()));
                }
                if !ids.contains(d.as_str()) {
                    return Err(PlanError::UnknownDependency {
                        task: t.id.clone(),
                        dependency: d.clone(),
                    });
                }
            }
        }
        self.execution_order().map(|_| ())?;

        let known: BTreeSet<&str> = spec.criteria.iter().map(|c| c.id.as_str()).collect();
        let mut covered = BTreeSet::new();
        for t in &self.tasks {
            for c in &t.criteria {
                if !known.contains(c.as_str()) {
                    return Err(PlanError::UnknownCriterion {
                        task: t.id.clone(),
                        criterion: c.clone(),
                    });
                }
                covered.insert(c.as_str());
            }
        }
        for c in &spec.criteria {
            if c.verification.needs_task() && !covered.contains(c.id.as_str()) {
                return Err(PlanError::UncoveredCriterion(c.id.clone()));
            }
        }
        Ok(())
    }

    /// A topological order that keeps the plan's own order among independent tasks.
    pub fn execution_order(&self) -> Result<Vec<&PlanTask>, PlanError> {
        let by_id: BTreeMap<&str, &PlanTask> =
            self.tasks.iter().map(|t| (t.id.as_str(), t)).collect();
        let mut done: BTreeSet<&str> = BTreeSet::new();
        let mut order = Vec::with_capacity(self.tasks.len());
        while order.len() < self.tasks.len() {
            let next = self.tasks.iter().find(|t| {
                !done.contains(t.id.as_str())
                    && t.depends_on
                        .iter()
                        .all(|d| by_id.contains_key(d.as_str()) && done.contains(d.as_str()))
            });
            match next {
                Some(t) => {
                    done.insert(t.id.as_str());
                    order.push(t);
                }
                None => {
                    let stuck = self
                        .tasks
                        .iter()
                        .find(|t| !done.contains(t.id.as_str()))
                        .map_or_else(String::new, |t| t.id.clone());
                    return Err(PlanError::Cycle(stuck));
                }
            }
        }
        Ok(order)
    }

    #[must_use]
    pub fn task_count(&self) -> usize {
        self.tasks.len()
    }
}

/// Kinds of stored artifacts (`docs/DOMAIN-MODEL.md` §3.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    DiscoveryReport,
    Specification,
    PreflightReport,
    Plan,
    ContextManifest,
    Patch,
    GateResult,
    ReviewReport,
    VerificationReport,
    Transcript,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn crit(id: &str, v: VerificationMethod) -> AcceptanceCriterion {
        AcceptanceCriterion {
            id: id.into(),
            statement: format!("{id} holds"),
            verification: v,
        }
    }

    fn spec() -> Specification {
        Specification {
            summary: "Add rate limiting".into(),
            in_scope: vec![],
            out_of_scope: vec![],
            criteria: vec![
                crit("AC-1", VerificationMethod::Test),
                crit("AC-2", VerificationMethod::Command),
                crit("AC-3", VerificationMethod::Review),
            ],
            affected_areas: vec!["src/http/**".into()],
            risks: vec![],
            assumptions: vec![],
        }
    }

    fn task(id: &str, criteria: &[&str], deps: &[&str]) -> PlanTask {
        PlanTask {
            id: id.into(),
            title: id.into(),
            instructions: "do it".into(),
            expected_paths: vec![],
            criteria: criteria.iter().map(ToString::to_string).collect(),
            depends_on: deps.iter().map(ToString::to_string).collect(),
            complexity: None,
        }
    }

    #[test]
    fn spec_validation() {
        assert_eq!(spec().validate(), Ok(()));
        assert_eq!(spec().criteria_count(), 3);
        let mut s = spec();
        s.summary = "  ".into();
        assert_eq!(s.validate(), Err(SpecError::EmptySummary));
        let mut s = spec();
        s.criteria.clear();
        assert_eq!(s.validate(), Err(SpecError::NoCriteria));
        let mut s = spec();
        s.criteria.push(crit("AC-1", VerificationMethod::Manual));
        assert_eq!(
            s.validate(),
            Err(SpecError::DuplicateCriterion("AC-1".into()))
        );
        let mut s = spec();
        s.criteria[0].id = "AC 1".into();
        assert_eq!(s.validate(), Err(SpecError::BadCriterionId("AC 1".into())));
        let mut s = spec();
        s.criteria[1].statement = String::new();
        assert_eq!(s.validate(), Err(SpecError::EmptyStatement("AC-2".into())));
    }

    #[test]
    fn unknown_fields_are_rejected() {
        let json = r#"{"summary":"s","criteria":[],"approved":true}"#;
        assert!(
            serde_json::from_str::<Specification>(json).is_err(),
            "agents cannot smuggle fields"
        );
    }

    #[test]
    fn plan_validation() {
        let good = Plan {
            tasks: vec![task("T1", &["AC-1"], &[]), task("T2", &["AC-2"], &["T1"])],
        };
        assert_eq!(
            good.validate(&spec()),
            Ok(()),
            "review-verified AC-3 needs no task"
        );

        assert_eq!(
            Plan { tasks: vec![] }.validate(&spec()),
            Err(PlanError::NoTasks)
        );
        let dup = Plan {
            tasks: vec![task("T1", &["AC-1", "AC-2"], &[]), task("T1", &[], &[])],
        };
        assert_eq!(
            dup.validate(&spec()),
            Err(PlanError::DuplicateTask("T1".into()))
        );
        let unknown_dep = Plan {
            tasks: vec![task("T1", &["AC-1", "AC-2"], &["T9"])],
        };
        assert!(matches!(
            unknown_dep.validate(&spec()),
            Err(PlanError::UnknownDependency { .. })
        ));
        let self_dep = Plan {
            tasks: vec![task("T1", &["AC-1", "AC-2"], &["T1"])],
        };
        assert_eq!(
            self_dep.validate(&spec()),
            Err(PlanError::SelfDependency("T1".into()))
        );
        let cycle = Plan {
            tasks: vec![task("A", &["AC-1"], &["B"]), task("B", &["AC-2"], &["A"])],
        };
        assert!(matches!(cycle.validate(&spec()), Err(PlanError::Cycle(_))));
        let bad_ref = Plan {
            tasks: vec![task("T1", &["AC-1", "AC-2", "AC-99"], &[])],
        };
        assert!(matches!(
            bad_ref.validate(&spec()),
            Err(PlanError::UnknownCriterion { .. })
        ));
        let uncovered = Plan {
            tasks: vec![task("T1", &["AC-1"], &[])],
        };
        assert_eq!(
            uncovered.validate(&spec()),
            Err(PlanError::UncoveredCriterion("AC-2".into()))
        );
    }

    #[test]
    fn execution_order_respects_dependencies_and_listed_order() {
        let plan = Plan {
            tasks: vec![
                task("C", &[], &["A", "B"]),
                task("A", &[], &[]),
                task("B", &[], &["A"]),
            ],
        };
        let order: Vec<_> = plan
            .execution_order()
            .expect("acyclic")
            .iter()
            .map(|t| t.id.as_str())
            .collect();
        assert_eq!(order, ["A", "B", "C"]);
    }
}
