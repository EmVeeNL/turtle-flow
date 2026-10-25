//! The run abstraction the executors use, and a scripted fake for tests and demos.
//!
//! `AgentRunner` is deliberately smaller than the full provider adapter in
//! `docs/AGENT-PROTOCOL.md`: a request goes in, structured output and usage come out. The real
//! Claude Code and Codex adapters (milestones M2/M3) implement the same trait on top of process
//! execution, so everything above it can be built and tested without an AI.

use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::path::PathBuf;
use std::sync::Mutex;

use serde_json::Value;
use turtle_domain::{
    AcceptanceCriterion, CriterionResult, CriterionStatus, Plan, PlanTask, ReviewReport, Role,
    RunId, Specification, TokenUsage, UsageValue, VerificationMethod, VerificationReport,
};
use turtle_exec::CommandSpec;

#[derive(Debug, Clone)]
pub struct AgentRequest {
    pub run_id: RunId,
    pub role: Role,
    pub job_key: String,
    pub title: String,
    pub description: String,
    /// The directory the run works in: the job worktree for code roles, the project checkout
    /// (read-only) for roles that run before a worktree exists.
    pub workdir: Option<PathBuf>,
    pub spec: Option<Specification>,
    pub plan: Option<Plan>,
    pub task: Option<PlanTask>,
    /// Gate output, review findings or user feedback the run must address.
    pub feedback: Option<String>,
    pub diff_summary: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AgentOutput {
    /// Output for roles with a structured contract (spec, plan, review, verification).
    pub structured: Option<Value>,
    pub text: String,
    pub usage: UsageValue,
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum AgentError {
    #[error("{0}")]
    Retryable(String),
    #[error("{0}")]
    Fatal(String),
}

pub trait AgentRunner: Send + Sync + 'static {
    /// Provider name recorded in commit trailers and run events.
    fn name(&self) -> &str {
        "agent"
    }

    /// Model recorded with each run; `"default"` when the CLI picks its own.
    fn model(&self) -> &str {
        "default"
    }

    fn run(
        &self,
        req: &AgentRequest,
    ) -> impl Future<Output = Result<AgentOutput, AgentError>> + Send;
}

/// What a scripted fake run does instead of its default behavior.
#[derive(Debug, Clone)]
pub enum Step {
    /// Make no changes (for code roles) or return an empty-but-valid result.
    Skip,
    /// Behave like a misbehaving agent: edit files, then run `git commit` itself.
    CommitInWorktree,
    /// Behave like a misbehaving agent: create a tag.
    TagInWorktree,
    WriteFile {
        path: String,
        content: String,
    },
    /// Return exactly this structured output.
    Output(Value),
    Fail(AgentError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedCall {
    pub role: Role,
    pub task: Option<String>,
    pub had_feedback: bool,
}

/// A deterministic agent: writes real files, returns valid documents, and can be told to fail,
/// skip work or misbehave once per scripted step.
#[derive(Debug, Default)]
pub struct FakeRunner {
    scripts: Mutex<HashMap<Role, VecDeque<Step>>>,
    calls: Mutex<Vec<RecordedCall>>,
}

impl FakeRunner {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Queues steps for `role`; each run of that role consumes one, then falls back to default.
    #[must_use]
    pub fn with_script(self, role: Role, steps: Vec<Step>) -> Self {
        self.scripts
            .lock()
            .expect("lock")
            .insert(role, steps.into());
        self
    }

    #[must_use]
    pub fn calls(&self) -> Vec<RecordedCall> {
        self.calls.lock().expect("lock").clone()
    }

    #[must_use]
    pub fn calls_of(&self, role: Role) -> usize {
        self.calls().iter().filter(|c| c.role == role).count()
    }

    fn next_step(&self, role: Role) -> Option<Step> {
        self.scripts
            .lock()
            .expect("lock")
            .get_mut(&role)
            .and_then(VecDeque::pop_front)
    }
}

fn usage() -> UsageValue {
    UsageValue::Estimated {
        usage: TokenUsage {
            input: 1_000,
            output: 200,
            ..TokenUsage::default()
        },
    }
}

fn json_output(value: &impl serde::Serialize, text: &str) -> AgentOutput {
    AgentOutput {
        structured: Some(serde_json::to_value(value).expect("fake documents serialize")),
        text: text.to_owned(),
        usage: usage(),
    }
}

fn feature_file(task: &PlanTask) -> String {
    format!("feature_{}.txt", task.id.to_ascii_lowercase())
}

fn write(workdir: &std::path::Path, rel: &str, content: &str) -> Result<(), AgentError> {
    let path = workdir.join(rel);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AgentError::Fatal(e.to_string()))?;
    }
    std::fs::write(path, content).map_err(|e| AgentError::Fatal(e.to_string()))
}

async fn git(workdir: &std::path::Path, args: &[&str]) -> Result<(), AgentError> {
    let spec = CommandSpec::new("git")
        .args(args.iter().copied())
        .cwd(workdir)
        .env("GIT_AUTHOR_NAME", "Fake Agent")
        .env("GIT_AUTHOR_EMAIL", "agent@fake.invalid")
        .env("GIT_COMMITTER_NAME", "Fake Agent")
        .env("GIT_COMMITTER_EMAIL", "agent@fake.invalid")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1");
    match turtle_exec::run(spec).await {
        Ok(out) if out.success() => Ok(()),
        Ok(out) => Err(AgentError::Fatal(
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )),
        Err(e) => Err(AgentError::Fatal(e.to_string())),
    }
}

impl AgentRunner for FakeRunner {
    fn name(&self) -> &str {
        "fake"
    }

    async fn run(&self, req: &AgentRequest) -> Result<AgentOutput, AgentError> {
        self.calls.lock().expect("lock").push(RecordedCall {
            role: req.role,
            task: req.task.as_ref().map(|t| t.id.clone()),
            had_feedback: req.feedback.is_some(),
        });
        let step = self.next_step(req.role);
        let workdir = req.workdir.as_deref();

        match &step {
            Some(Step::Fail(e)) => return Err(e.clone()),
            Some(Step::Output(v)) => {
                return Ok(AgentOutput {
                    structured: Some(v.clone()),
                    text: "scripted output".into(),
                    usage: usage(),
                });
            }
            _ => {}
        }

        match req.role {
            Role::Discovery => Ok(json_output(
                &serde_json::json!({"summary": "fake discovery", "relevant_files": [], "risks": []}),
                "discovery done",
            )),
            Role::SpecWriter => Ok(json_output(
                &Specification {
                    summary: format!("{} ({})", req.title, req.job_key),
                    in_scope: vec![req.title.clone()],
                    out_of_scope: vec![],
                    criteria: vec![AcceptanceCriterion {
                        id: "AC-1".into(),
                        statement: "The feature is implemented and the project's checks pass"
                            .into(),
                        verification: VerificationMethod::Test,
                    }],
                    affected_areas: vec!["**".into()],
                    risks: vec![],
                    assumptions: vec![],
                },
                "spec written",
            )),
            Role::Planner => {
                let criteria: Vec<String> = req
                    .spec
                    .as_ref()
                    .map(|s| {
                        s.criteria
                            .iter()
                            .filter(|c| c.verification.needs_task())
                            .map(|c| c.id.clone())
                            .collect()
                    })
                    .unwrap_or_default();
                Ok(json_output(
                    &Plan {
                        tasks: vec![PlanTask {
                            id: "T1".into(),
                            title: "Implement the feature".into(),
                            instructions: req.description.clone(),
                            expected_paths: vec!["feature_*.txt".into()],
                            criteria,
                            depends_on: vec![],
                            complexity: None,
                        }],
                    },
                    "plan written",
                ))
            }
            Role::Implementer | Role::Fixer => {
                let dir =
                    workdir.ok_or_else(|| AgentError::Fatal("code roles need a workdir".into()))?;
                match step {
                    Some(Step::Skip) => {}
                    Some(Step::WriteFile { path, content }) => write(dir, &path, &content)?,
                    Some(Step::CommitInWorktree) => {
                        write(dir, "sneaky.txt", "committed by the agent\n")?;
                        git(dir, &["add", "-A"]).await?;
                        git(dir, &["commit", "-q", "-m", "agent commits on its own"]).await?;
                    }
                    Some(Step::TagInWorktree) => git(dir, &["tag", "agent-was-here"]).await?,
                    _ => {
                        if let Some(task) = &req.task {
                            write(
                                dir,
                                &feature_file(task),
                                &format!("implemented: {}\n", task.title),
                            )?;
                        }
                        // A fixer repairs whatever the plan promised but is missing.
                        if req.role == Role::Fixer
                            && let Some(plan) = &req.plan
                        {
                            for task in &plan.tasks {
                                let rel = feature_file(task);
                                if !dir.join(&rel).exists() {
                                    write(
                                        dir,
                                        &rel,
                                        &format!("implemented (fixed): {}\n", task.title),
                                    )?;
                                }
                            }
                        }
                    }
                }
                Ok(AgentOutput {
                    structured: None,
                    text: "changes made".into(),
                    usage: usage(),
                })
            }
            Role::Reviewer => Ok(json_output(
                &ReviewReport {
                    summary: "fake review: nothing to report".into(),
                    ..ReviewReport::default()
                },
                "review done",
            )),
            Role::Verifier => {
                let criteria = req
                    .spec
                    .as_ref()
                    .map(|s| {
                        s.criteria
                            .iter()
                            .map(|c| CriterionResult {
                                id: c.id.clone(),
                                status: CriterionStatus::Met,
                                evidence: "fake verification".into(),
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                Ok(json_output(
                    &VerificationReport {
                        criteria,
                        structural: false,
                    },
                    "verified",
                ))
            }
            Role::Summarizer | Role::ReleaseNotes => Ok(AgentOutput {
                structured: None,
                text: "ok".into(),
                usage: usage(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(role: Role, workdir: Option<PathBuf>) -> AgentRequest {
        AgentRequest {
            run_id: RunId::new(),
            role,
            job_key: "API-1".into(),
            title: "Add thing".into(),
            description: "details".into(),
            workdir,
            spec: None,
            plan: None,
            task: Some(PlanTask {
                id: "T1".into(),
                title: "do it".into(),
                instructions: String::new(),
                expected_paths: vec![],
                criteria: vec![],
                depends_on: vec![],
                complexity: None,
            }),
            feedback: None,
            diff_summary: None,
        }
    }

    #[tokio::test]
    async fn default_documents_are_valid() {
        let fake = FakeRunner::new();
        let spec_out = fake
            .run(&request(Role::SpecWriter, None))
            .await
            .expect("spec");
        let spec: Specification =
            serde_json::from_value(spec_out.structured.expect("structured")).expect("parse");
        assert_eq!(spec.validate(), Ok(()));

        let mut req = request(Role::Planner, None);
        req.spec = Some(spec.clone());
        let plan: Plan = serde_json::from_value(
            fake.run(&req)
                .await
                .expect("plan")
                .structured
                .expect("structured"),
        )
        .expect("parse");
        assert_eq!(plan.validate(&spec), Ok(()));
    }

    #[tokio::test]
    async fn implementer_writes_a_real_file_and_scripts_are_consumed_once() {
        let dir = tempfile::tempdir().expect("tempdir");
        let fake = FakeRunner::new().with_script(Role::Implementer, vec![Step::Skip]);
        let req = request(Role::Implementer, Some(dir.path().to_path_buf()));
        fake.run(&req).await.expect("skip");
        assert!(
            !dir.path().join("feature_t1.txt").exists(),
            "scripted step skipped the work"
        );
        fake.run(&req).await.expect("default");
        assert!(
            dir.path().join("feature_t1.txt").exists(),
            "then default behavior resumes"
        );
        assert_eq!(fake.calls_of(Role::Implementer), 2);
    }

    #[tokio::test]
    async fn scripted_failures_and_outputs() {
        let fake = FakeRunner::new()
            .with_script(
                Role::Reviewer,
                vec![Step::Fail(AgentError::Retryable("503".into()))],
            )
            .with_script(
                Role::Verifier,
                vec![Step::Output(serde_json::json!({"criteria": []}))],
            );
        assert!(matches!(
            fake.run(&request(Role::Reviewer, None)).await,
            Err(AgentError::Retryable(_))
        ));
        let out = fake
            .run(&request(Role::Verifier, None))
            .await
            .expect("scripted");
        assert_eq!(out.structured, Some(serde_json::json!({"criteria": []})));
    }

    #[tokio::test]
    async fn code_roles_require_a_workdir() {
        let err = FakeRunner::new()
            .run(&request(Role::Implementer, None))
            .await
            .expect_err("no workdir");
        assert!(matches!(err, AgentError::Fatal(_)));
    }
}
