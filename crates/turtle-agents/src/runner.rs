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
    /// Redacted repository context and its auditable manifest, built by `turtle-context`.
    pub context: Option<AgentContext>,
}

#[derive(Debug, Clone)]
pub struct AgentContext {
    pub text: String,
    pub manifest_json: String,
}

#[derive(Debug, Clone)]
pub struct AgentOutput {
    /// Output for roles with a structured contract (spec, plan, review, verification).
    pub structured: Option<Value>,
    pub text: String,
    pub usage: UsageValue,
    /// Original provider stream; redacted and capped by the executor before storage.
    pub transcript: Option<String>,
    pub permission_denials: Vec<crate::claude::PermissionDenial>,
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum AgentError {
    #[error("{0}")]
    Retryable(String),
    #[error("{0}")]
    Fatal(String),
    #[error("{0}")]
    RetryableDetails(Box<AgentErrorDetails>),
    #[error("{0}")]
    FatalDetails(Box<AgentErrorDetails>),
}

#[derive(Debug, Clone)]
pub struct AgentErrorDetails {
    message: String,
    usage: Option<UsageValue>,
    transcript: Option<String>,
    permission_denials: Vec<crate::claude::PermissionDenial>,
}

impl std::fmt::Display for AgentErrorDetails {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl AgentError {
    #[must_use]
    pub fn retryable(&self) -> bool {
        matches!(self, Self::Retryable(_) | Self::RetryableDetails(_))
    }
    #[must_use]
    pub fn usage(&self) -> Option<UsageValue> {
        match self {
            Self::RetryableDetails(details) | Self::FatalDetails(details) => details.usage,
            _ => None,
        }
    }
    #[must_use]
    pub fn transcript(&self) -> Option<&str> {
        match self {
            Self::RetryableDetails(details) | Self::FatalDetails(details) => {
                details.transcript.as_deref()
            }
            _ => None,
        }
    }
    #[must_use]
    pub fn permission_denials(&self) -> &[crate::claude::PermissionDenial] {
        match self {
            Self::RetryableDetails(details) | Self::FatalDetails(details) => {
                &details.permission_denials
            }
            _ => &[],
        }
    }
    #[must_use]
    pub fn with_details(
        self,
        usage: Option<UsageValue>,
        transcript: Option<String>,
        permission_denials: Vec<crate::claude::PermissionDenial>,
    ) -> Self {
        let (retryable, message) = match self {
            Self::Retryable(message) => (true, message),
            Self::Fatal(message) => (false, message),
            Self::RetryableDetails(details) => (true, details.message),
            Self::FatalDetails(details) => (false, details.message),
        };
        let details = Box::new(AgentErrorDetails {
            message,
            usage,
            transcript,
            permission_denials,
        });
        if retryable {
            Self::RetryableDetails(details)
        } else {
            Self::FatalDetails(details)
        }
    }
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

    /// Resolve a provider before a run is reserved. Single-provider runners retain their
    /// previous behavior; a routing runner overrides this and `run_selected`.
    fn select(&self, context: &RouteContext) -> Result<RouteSelection, AgentError> {
        let independent = matches!(context.role, Role::Reviewer | Role::Verifier)
            && context.implementer_provider.as_deref() == Some(self.name());
        if independent && context.independence == Independence::Required {
            return Err(AgentError::Fatal(
                "independent review requires another available provider".into(),
            ));
        }
        Ok(RouteSelection {
            provider: self.name().to_owned(),
            model: self.model().to_owned(),
            candidates: serde_json::json!([{"provider": self.name(), "model": self.model(), "eligible": true}]),
            reason: if independent && context.independence == Independence::Preferred {
                "independent provider unavailable; preferred review fell back to the implementer provider".into()
            } else {
                format!("single provider for {:?}", context.role)
            },
            override_used: false,
        })
    }

    fn run_selected(
        &self,
        req: &AgentRequest,
        _provider: &str,
    ) -> impl Future<Output = Result<AgentOutput, AgentError>> + Send {
        self.run(req)
    }

    fn run(
        &self,
        req: &AgentRequest,
    ) -> impl Future<Output = Result<AgentOutput, AgentError>> + Send;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Independence {
    Off,
    Preferred,
    Required,
}

#[derive(Debug, Clone)]
pub struct RouteContext {
    pub role: Role,
    pub independence: Independence,
    pub implementer_provider: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RouteSelection {
    pub provider: String,
    pub model: String,
    pub candidates: Value,
    pub reason: String,
    pub override_used: bool,
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
    reported_usage: bool,
    usage_scripts: Mutex<HashMap<Role, VecDeque<UsageValue>>>,
}

impl FakeRunner {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn with_reported_usage(mut self) -> Self {
        self.reported_usage = true;
        self
    }

    #[must_use]
    pub fn with_usage_script(self, role: Role, usage: Vec<UsageValue>) -> Self {
        self.usage_scripts
            .lock()
            .expect("lock")
            .insert(role, usage.into());
        self
    }

    fn next_usage(&self, role: Role) -> UsageValue {
        self.usage_scripts
            .lock()
            .expect("lock")
            .get_mut(&role)
            .and_then(VecDeque::pop_front)
            .unwrap_or_else(|| self.usage())
    }

    fn usage(&self) -> UsageValue {
        let usage = TokenUsage {
            input: 1_000,
            output: 200,
            ..TokenUsage::default()
        };
        if self.reported_usage {
            UsageValue::Reported { usage }
        } else {
            UsageValue::Estimated { usage }
        }
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

fn json_output(value: &impl serde::Serialize, text: &str, usage: UsageValue) -> AgentOutput {
    AgentOutput {
        structured: Some(serde_json::to_value(value).expect("fake documents serialize")),
        text: text.to_owned(),
        usage,
        transcript: None,
        permission_denials: Vec::new(),
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
        let usage = self.next_usage(req.role);
        let workdir = req.workdir.as_deref();

        match &step {
            Some(Step::Fail(e)) => return Err(e.clone()),
            Some(Step::Output(v)) => {
                return Ok(AgentOutput {
                    structured: Some(v.clone()),
                    text: "scripted output".into(),
                    usage,
                    transcript: None,
                    permission_denials: Vec::new(),
                });
            }
            _ => {}
        }

        match req.role {
            Role::Discovery => Ok(json_output(
                &serde_json::json!({"summary": "fake discovery", "relevant_files": [], "risks": []}),
                "discovery done",
                usage,
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
                usage,
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
                    usage,
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
                    usage,
                    transcript: None,
                    permission_denials: Vec::new(),
                })
            }
            Role::Reviewer => Ok(json_output(
                &ReviewReport {
                    summary: "fake review: nothing to report".into(),
                    ..ReviewReport::default()
                },
                "review done",
                usage,
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
                    usage,
                ))
            }
            Role::Summarizer | Role::ReleaseNotes => Ok(AgentOutput {
                structured: None,
                text: "ok".into(),
                usage,
                transcript: None,
                permission_denials: Vec::new(),
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
            context: None,
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

    #[test]
    fn a_single_runner_obeys_required_review_independence() {
        let runner = FakeRunner::new();
        let context = RouteContext {
            role: Role::Reviewer,
            independence: Independence::Required,
            implementer_provider: Some("fake".into()),
        };
        let err = runner
            .select(&context)
            .expect_err("no independent provider");
        assert!(
            err.to_string()
                .contains("requires another available provider")
        );
        let preferred = runner
            .select(&RouteContext {
                independence: Independence::Preferred,
                ..context
            })
            .expect("fallback");
        assert!(preferred.reason.contains("fell back"));
    }
}
