//! Isolated headless Codex runs, following the recorded S-2 CLI contract.

use std::path::PathBuf;
use std::time::Duration;

use serde_json::Value;
use turtle_domain::{Role, TokenUsage, UsageValue};
use turtle_exec::{CommandSpec, ExecError, ExitOutcome, Output};

use crate::codex::{CodexError, ISOLATION_FLAGS, classify_error_event};
use crate::prompts;
use crate::runner::{AgentError, AgentOutput, AgentRequest, AgentRunner};
use crate::schemas;

#[derive(Debug, Clone)]
pub struct CodexConfig {
    pub program: PathBuf,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub timeout: Duration,
    /// Only these variables are inherited beyond turtle-exec's small base allowlist.
    pub inherit_env: Vec<String>,
}

impl Default for CodexConfig {
    fn default() -> Self {
        Self {
            program: PathBuf::from("codex"),
            model: None,
            effort: None,
            timeout: Duration::from_secs(30 * 60),
            inherit_env: ["CODEX_HOME", "OPENAI_API_KEY"].map(String::from).to_vec(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct CodexRunner {
    config: CodexConfig,
}

impl CodexRunner {
    #[must_use]
    pub fn new(config: CodexConfig) -> Self {
        Self { config }
    }

    #[must_use]
    pub fn args(
        &self,
        role: Role,
        schema: Option<&std::path::Path>,
        dir: &std::path::Path,
    ) -> Vec<String> {
        let mut args: Vec<String> = ISOLATION_FLAGS.iter().map(|s| (*s).to_owned()).collect();
        if let Some(model) = &self.config.model {
            args.extend(["-m".into(), model.clone()]);
        }
        if let Some(effort) = &self.config.effort {
            args.extend(["-c".into(), format!("model_reasoning_effort={effort}")]);
        }
        args.extend([
            "-s".into(),
            if role.writes_files() {
                "workspace-write"
            } else {
                "read-only"
            }
            .into(),
        ]);
        if let Some(schema) = schema {
            args.extend([
                "--output-schema".into(),
                schema.to_string_lossy().into_owned(),
            ]);
        }
        args.extend(["-C".into(), dir.to_string_lossy().into_owned(), "-".into()]);
        args
    }
}

impl AgentRunner for CodexRunner {
    fn name(&self) -> &str {
        "codex"
    }

    fn model(&self) -> &str {
        self.config.model.as_deref().unwrap_or("default")
    }

    async fn run(&self, req: &AgentRequest) -> Result<AgentOutput, AgentError> {
        let workdir = req
            .workdir
            .as_deref()
            .ok_or_else(|| AgentError::Fatal("a Codex run needs a working directory".into()))?;
        let scratch = tempfile::tempdir()
            .map_err(|e| AgentError::Fatal(format!("cannot create schema directory: {e}")))?;
        let schema_path = if let Some(schema) = schemas::codex_for_role(req.role) {
            let path = scratch.path().join("output-schema.json");
            std::fs::write(&path, schema.to_string())
                .map_err(|e| AgentError::Fatal(format!("cannot write Codex schema: {e}")))?;
            Some(path)
        } else {
            None
        };
        let mut spec = CommandSpec::new(&self.config.program)
            .args(self.args(req.role, schema_path.as_deref(), workdir))
            .cwd(workdir)
            .stdin(prompts::build(req))
            .timeout(self.config.timeout);
        spec.inherit_env.clone_from(&self.config.inherit_env);
        let output = match turtle_exec::run(spec).await {
            Ok(o) => o,
            Err(ExecError::NotFound(p)) => {
                return Err(AgentError::Fatal(format!(
                    "`{p}` was not found; install Codex or fix the path"
                )));
            }
            Err(e) => return Err(AgentError::Retryable(e.to_string())),
        };
        interpret(&output, req.role)
    }
}

#[derive(Default)]
struct Summary {
    final_message: Option<String>,
    usage: Option<TokenUsage>,
    error: Option<CodexError>,
    completed: bool,
}

impl Summary {
    fn on_line(&mut self, line: &str) {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            return;
        };
        if let Some(error) = classify_error_event(line) {
            self.error = Some(error);
        }
        match value["type"].as_str() {
            Some("item.completed") if value["item"]["type"] == "agent_message" => {
                self.final_message = value["item"]["text"].as_str().map(str::to_owned);
            }
            Some("turn.completed") => {
                self.completed = true;
                let raw = &value["usage"];
                if let (Some(input), Some(output)) =
                    (raw["input_tokens"].as_u64(), raw["output_tokens"].as_u64())
                {
                    let cached = raw["cached_input_tokens"].as_u64().unwrap_or(0);
                    // Reject inconsistent usage rather than charging fabricated totals.
                    self.usage = input.checked_sub(cached).map(|uncached| TokenUsage {
                        input: uncached,
                        cached_input_read: raw["cached_input_tokens"].as_u64(),
                        cached_input_write: raw["cache_write_input_tokens"].as_u64(),
                        output,
                        reasoning: raw["reasoning_output_tokens"].as_u64(),
                    });
                }
            }
            _ => {}
        }
    }
}

/// Pure interpretation of a completed process, including partial usage on failures.
pub fn interpret(out: &Output, role: Role) -> Result<AgentOutput, AgentError> {
    let stdout = out.stdout_lossy();
    let stderr = String::from_utf8_lossy(&out.stderr);
    let raw = format!(
        "{stdout}{}{}",
        if stderr.is_empty() {
            ""
        } else {
            "\n[stderr]\n"
        },
        stderr
    );
    let transcript = (!raw.is_empty()).then_some(raw);
    let mut summary = Summary::default();
    for line in stdout.lines() {
        summary.on_line(line);
    }
    let usage = summary.usage.map(|usage| UsageValue::Reported { usage });
    let detail = |error: AgentError| error.with_details(usage, transcript.clone(), Vec::new());
    match out.outcome {
        ExitOutcome::TimedOut => {
            return Err(detail(AgentError::Fatal(format!(
                "the run was stopped after {}s",
                out.duration.as_secs()
            ))));
        }
        ExitOutcome::Signaled => {
            return Err(detail(AgentError::Retryable(
                "Codex was killed by a signal".into(),
            )));
        }
        ExitOutcome::Exited(_) => {}
    }
    if out.truncated {
        return Err(detail(AgentError::Fatal(
            "Codex produced more output than the limit allows".into(),
        )));
    }
    if !out.success() || summary.error.is_some() {
        let (retryable, message) = match summary.error {
            Some(CodexError::UsageLimit { message, retry_at }) => (
                true,
                format!(
                    "{message}{}",
                    retry_at.map_or(String::new(), |(h, m)| format!(
                        " (retry hint {h:02}:{m:02} local time)"
                    ))
                ),
            ),
            Some(CodexError::Other { message }) => (transient(&message), message),
            None => {
                let message = if stderr.trim().is_empty() {
                    "Codex failed without saying why".into()
                } else {
                    stderr.trim().to_owned()
                };
                (transient(&message), message)
            }
        };
        return Err(detail(if retryable {
            AgentError::Retryable(message)
        } else {
            AgentError::Fatal(message)
        }));
    }
    if !summary.completed {
        return Err(detail(AgentError::Retryable(
            "Codex ended without turn.completed".into(),
        )));
    }
    let text = summary.final_message.ok_or_else(|| {
        detail(AgentError::Retryable(
            "Codex ended without a final agent message".into(),
        ))
    })?;
    let structured = if schemas::for_role(role).is_some() {
        Some(serde_json::from_str(&text).map_err(|_| {
            detail(AgentError::Retryable(
                "Codex did not return the requested structured document".into(),
            ))
        })?)
    } else {
        None
    };
    Ok(AgentOutput {
        structured,
        text,
        usage: usage.unwrap_or(UsageValue::Unavailable),
        transcript,
        permission_denials: Vec::new(),
    })
}

fn transient(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    [
        "429",
        "500",
        "502",
        "503",
        "overloaded",
        "rate limit",
        "temporarily",
        "connection",
        "timed out",
        "try again",
    ]
    .iter()
    .any(|part| lower.contains(part))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str, status: i32) -> Output {
        Output {
            outcome: ExitOutcome::Exited(status),
            stdout: std::fs::read(format!(
                "{}/../../spikes/fixtures/codex/{name}",
                env!("CARGO_MANIFEST_DIR")
            ))
            .expect("fixture"),
            stderr: Vec::new(),
            truncated: false,
            duration: Duration::from_secs(1),
        }
    }

    #[test]
    fn recorded_usage_subtracts_cached_input_once() {
        let out = interpret(
            &fixture("02-readonly-structured.ndjson", 0),
            Role::Discovery,
        )
        .expect("success");
        assert!(
            out.structured.expect("document")["summary"]
                .as_str()
                .expect("summary")
                .starts_with("This is a minimal Rust scratch repository.")
        );
        let UsageValue::Reported { usage } = out.usage else {
            panic!("reported usage expected")
        };
        assert_eq!(
            (usage.input, usage.cached_input_read, usage.output),
            (18_083, Some(34_432), 214)
        );
        assert_eq!(usage.total(), 52_729);
    }

    #[test]
    fn error_and_implementation_fixtures() {
        let err = interpret(&fixture("01-usage-limit-error.ndjson", 1), Role::Discovery)
            .expect_err("usage limit");
        assert!(err.retryable() && err.to_string().contains("15:36"));
        assert!(
            err.transcript()
                .expect("transcript")
                .contains("turn.failed")
        );
        assert_eq!(err.usage(), None);
        let out = interpret(
            &fixture("03-implementer-commit-attempt.ndjson", 0),
            Role::Implementer,
        )
        .expect("success");
        assert!(out.text.contains("index.lock"));
        assert!(
            matches!(out.usage, UsageValue::Reported { usage } if usage.input == 3759 && usage.cached_input_read == Some(26240) && usage.total() == 30199)
        );
    }

    #[test]
    fn failed_turn_keeps_reported_usage_and_does_not_double_count_cache() {
        let mut out = fixture("03-implementer-commit-attempt.ndjson", 1);
        out.stdout.extend_from_slice(b"\n{\"type\":\"turn.failed\",\"error\":{\"message\":\"temporary connection failure\"}}\n");
        let err = interpret(&out, Role::Implementer).expect_err("failed turn");
        assert!(err.retryable());
        assert!(
            matches!(err.usage(), Some(UsageValue::Reported { usage }) if usage.input == 3759 && usage.total() == 30199)
        );
        assert!(
            err.transcript()
                .expect("transcript")
                .contains("turn.failed")
        );
    }

    #[test]
    fn role_arguments_select_the_expected_sandbox() {
        let runner = CodexRunner::default();
        let dir = std::path::Path::new("/tmp/work");
        for role in [Role::Implementer, Role::Fixer] {
            assert!(
                runner
                    .args(role, None, dir)
                    .windows(2)
                    .any(|w| w == ["-s", "workspace-write"])
            );
        }
        for role in [
            Role::Discovery,
            Role::SpecWriter,
            Role::Planner,
            Role::Reviewer,
            Role::Verifier,
            Role::Summarizer,
            Role::ReleaseNotes,
        ] {
            assert!(
                runner
                    .args(role, None, dir)
                    .windows(2)
                    .any(|w| w == ["-s", "read-only"])
            );
        }
    }
}
