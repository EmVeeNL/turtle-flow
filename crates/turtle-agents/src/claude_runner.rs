//! Runs roles through the Claude Code CLI in headless mode (`docs/AGENT-PROTOCOL.md` §9.1).
//!
//! Every run carries the full isolation flag set (spike S-1 §1.3), a role-specific tool list,
//! a JSON schema for the roles with a structured contract, and a spend cap. The process runner
//! scrubs the environment; only the variables in [`ClaudeConfig::inherit_env`] pass through.

use std::path::PathBuf;
use std::time::Duration;

use turtle_domain::{Role, UsageValue};
use turtle_exec::{CommandSpec, ExecError, ExitOutcome, Output};

use crate::claude::{ISOLATION_FLAGS, RunSummary, StreamParser};
use crate::prompts;
use crate::runner::{AgentError, AgentOutput, AgentRequest, AgentRunner};
use crate::schemas;

/// Git commands an implementer must never run itself; Turtle Flow owns the repository history.
const DENIED_GIT: &[&str] = &[
    "commit",
    "push",
    "tag",
    "checkout",
    "switch",
    "branch",
    "reset",
    "rebase",
    "merge",
    "cherry-pick",
    "stash",
    "restore",
    "clean",
    "worktree",
];

#[derive(Debug, Clone)]
pub struct ClaudeConfig {
    pub program: PathBuf,
    pub model: Option<String>,
    /// `low`, `medium`, `high`, `xhigh` or `max`; unset uses the CLI default.
    pub effort: Option<String>,
    /// Hard stop per run, in API-equivalent USD (`--max-budget-usd`).
    pub max_budget_usd: Option<f64>,
    pub timeout: Duration,
    /// Variables copied from the daemon's environment. Authentication comes from the user's
    /// Claude Code login (found through `HOME`) unless one of these carries an API key.
    pub inherit_env: Vec<String>,
}

impl Default for ClaudeConfig {
    fn default() -> Self {
        Self {
            program: PathBuf::from("claude"),
            model: None,
            effort: None,
            max_budget_usd: Some(3.0),
            timeout: Duration::from_secs(30 * 60),
            inherit_env: [
                "ANTHROPIC_API_KEY",
                "CLAUDE_CODE_OAUTH_TOKEN",
                "CLAUDE_CONFIG_DIR",
            ]
            .map(String::from)
            .to_vec(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ClaudeRunner {
    config: ClaudeConfig,
}

impl ClaudeRunner {
    #[must_use]
    pub fn new(config: ClaudeConfig) -> Self {
        Self { config }
    }

    /// The full argument list for one run (the prompt goes to stdin, not here).
    #[must_use]
    pub fn args(&self, role: Role) -> Vec<String> {
        let mut args: Vec<String> = ISOLATION_FLAGS.iter().map(|s| (*s).to_owned()).collect();
        let cfg = &self.config;
        if let Some(model) = &cfg.model {
            args.extend(["--model".into(), model.clone()]);
        }
        if let Some(effort) = &cfg.effort {
            args.extend(["--effort".into(), effort.clone()]);
        }
        if let Some(usd) = cfg.max_budget_usd {
            args.extend(["--max-budget-usd".into(), format!("{usd:.2}")]);
        }
        if role.writes_files() {
            args.extend([
                "--tools".into(),
                "Read,Grep,Glob,Edit,Write,Bash".into(),
                "--permission-mode".into(),
                "acceptEdits".into(),
            ]);
            for sub in DENIED_GIT {
                args.extend(["--disallowedTools".into(), format!("Bash(git {sub} *)")]);
            }
        } else {
            args.extend(["--tools".into(), "Read,Grep,Glob".into()]);
        }
        if let Some(schema) = schemas::for_role(role) {
            args.extend(["--json-schema".into(), schema.to_string()]);
        }
        args
    }
}

impl AgentRunner for ClaudeRunner {
    fn name(&self) -> &str {
        "claude-code"
    }

    fn model(&self) -> &str {
        self.config.model.as_deref().unwrap_or("default")
    }

    async fn run(&self, req: &AgentRequest) -> Result<AgentOutput, AgentError> {
        let workdir = req.workdir.clone().ok_or_else(|| {
            AgentError::Fatal("a Claude Code run needs a working directory".into())
        })?;
        let mut spec = CommandSpec::new(&self.config.program)
            .args(self.args(req.role))
            .cwd(workdir)
            .stdin(prompts::build(req))
            .timeout(self.config.timeout);
        spec.inherit_env.clone_from(&self.config.inherit_env);
        let output = match turtle_exec::run(spec).await {
            Ok(o) => o,
            Err(ExecError::NotFound(p)) => {
                return Err(AgentError::Fatal(format!(
                    "`{p}` was not found; install Claude Code or fix the path"
                )));
            }
            Err(e) => return Err(AgentError::Retryable(e.to_string())),
        };
        interpret(&output, req.role)
    }
}

/// Words in an error that mean "try again later" rather than "this will never work".
const TRANSIENT: &[&str] = &[
    "overloaded",
    "529",
    "503",
    "502",
    "500",
    "rate limit",
    "usage limit",
    "429",
    "timed out",
    "econnreset",
    "etimedout",
    "temporarily",
    "try again",
    "connection",
];

fn tail(text: &str) -> String {
    let text = text.trim();
    let start = text.len().saturating_sub(600);
    let start = (start..=text.len())
        .find(|&i| text.is_char_boundary(i))
        .unwrap_or(text.len());
    text[start..].to_owned()
}

/// Turns the finished process into an outcome. Pure, so every failure mode is unit-tested.
pub fn interpret(out: &Output, role: Role) -> Result<AgentOutput, AgentError> {
    let raw = format!(
        "{}{}{}",
        String::from_utf8_lossy(&out.stdout),
        if out.stderr.is_empty() {
            ""
        } else {
            "\n[stderr]\n"
        },
        String::from_utf8_lossy(&out.stderr)
    );
    let transcript = (!raw.is_empty()).then_some(raw);
    let mut parser = StreamParser::new();
    for line in out.stdout_lossy().lines() {
        parser.on_line(line);
    }
    let summary = parser.finish();
    let usage = summary.usage.map(|usage| UsageValue::Reported { usage });
    let denials = summary.permission_denials.clone();
    match out.outcome {
        ExitOutcome::TimedOut => {
            return Err(AgentError::Fatal(format!(
                "the run was stopped after {}s",
                out.duration.as_secs()
            ))
            .with_details(usage, transcript, denials)
            .with_quota(summary.quota.clone()));
        }
        ExitOutcome::Signaled => {
            return Err(
                AgentError::Retryable("Claude Code was killed by a signal".into())
                    .with_details(usage, transcript, denials)
                    .with_quota(summary.quota.clone()),
            );
        }
        ExitOutcome::Exited(_) => {}
    }
    if out.truncated {
        return Err(AgentError::Fatal(
            "Claude Code produced more output than the limit allows".into(),
        )
        .with_details(usage, transcript, denials)
        .with_quota(summary.quota.clone()));
    }

    let leaks = summary.isolation_leaks();
    if !leaks.is_empty() {
        return Err(AgentError::Fatal(format!(
            "isolation failed: unexpected tools or servers were loaded ({})",
            leaks.join(", ")
        ))
        .with_details(usage, transcript, denials)
        .with_quota(summary.quota.clone()));
    }

    let quota_rejected = matches!(
        &summary.quota,
        Some(turtle_domain::QuotaStatus::Partial { windows })
            if windows.iter().any(|window| window.status == turtle_domain::WindowStatus::Rejected)
    );
    let limit_text = summary.result_text.as_deref().is_some_and(|text| {
        let lower = text.to_ascii_lowercase();
        lower.contains("usage limit")
            || lower.contains("rate limit reached")
            || lower.contains("rate limit has been reached")
    });
    let failed = summary.is_error == Some(true) || !out.success() || quota_rejected || limit_text;
    if failed {
        let detail = summary
            .result_text
            .clone()
            .filter(|t| !t.trim().is_empty())
            .unwrap_or_else(|| String::from_utf8_lossy(&out.stderr).into_owned());
        let detail = tail(&detail);
        let lower = detail.to_ascii_lowercase();
        let message = if detail.is_empty() {
            "Claude Code failed without saying why".to_owned()
        } else {
            format!("Claude Code failed: {detail}")
        };
        let rejected = quota_rejected;
        let explicit_limit = lower.contains("usage limit")
            || lower.contains("rate limit reached")
            || lower.contains("rate limit has been reached");
        if rejected || explicit_limit {
            let reset_at = match &summary.quota {
                Some(turtle_domain::QuotaStatus::Partial { windows }) => windows
                    .iter()
                    .filter(|window| window.status == turtle_domain::WindowStatus::Rejected)
                    .filter_map(|window| window.resets_at)
                    .min()
                    .and_then(|seconds| time::OffsetDateTime::from_unix_timestamp(seconds).ok()),
                _ => None,
            };
            return Err(AgentError::rate_limited(
                "claude-code",
                reset_at,
                message,
                usage,
                transcript,
                denials,
                summary.quota,
            ));
        }
        return Err(if TRANSIENT.iter().any(|t| lower.contains(t)) {
            AgentError::Retryable(message)
        } else {
            AgentError::Fatal(message)
        }
        .with_details(usage, transcript, denials)
        .with_quota(summary.quota.clone()));
    }
    if summary.result_text.is_none() && summary.structured_output.is_none() {
        return Err(
            AgentError::Retryable("Claude Code ended without a result".into())
                .with_details(usage, transcript, denials)
                .with_quota(summary.quota.clone()),
        );
    }

    let structured = if schemas::for_role(role).is_some() {
        Some(structured_output(&summary).map_err(|e| e.with_quota(summary.quota.clone()))?)
    } else {
        None
    };
    Ok(AgentOutput {
        structured,
        text: summary.result_text.unwrap_or_default(),
        usage: usage.unwrap_or(UsageValue::Unavailable),
        transcript,
        permission_denials: denials,
        quota: summary.quota,
    })
}

/// The schema-checked object, or the result text when it parses as JSON (older CLI builds).
fn structured_output(summary: &RunSummary) -> Result<serde_json::Value, AgentError> {
    if let Some(v) = &summary.structured_output {
        return Ok(v.clone());
    }
    let text = summary.result_text.as_deref().unwrap_or_default().trim();
    let text = text
        .strip_prefix("```json")
        .or_else(|| text.strip_prefix("```"))
        .map_or(text, |t| t.trim_end_matches("```").trim());
    serde_json::from_str(text).map_err(|_| {
        AgentError::Retryable(
            "the run did not return the structured document that was asked for".into(),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn output(stdout: &str, outcome: ExitOutcome) -> Output {
        Output {
            outcome,
            stdout: stdout.as_bytes().to_vec(),
            stderr: Vec::new(),
            truncated: false,
            duration: Duration::from_secs(3),
        }
    }

    fn fixture(name: &str) -> String {
        std::fs::read_to_string(format!(
            "{}/../../docs/spikes/fixtures/claude-code/{name}",
            env!("CARGO_MANIFEST_DIR")
        ))
        .expect("fixture")
    }

    #[test]
    fn rejected_quota_event_and_limit_result_are_rate_limited_but_529_is_retryable() {
        let stream = concat!(
            r#"{"type":"rate_limit_event","rate_limit_info":{"unifiedWindows":{"five_hour":{"utilization":1.0,"resetsAt":1792950000,"status":"rejected"}}}}"#,
            "\n",
            r#"{"type":"result","is_error":true,"result":"Usage limit reached","usage":{"input_tokens":12,"output_tokens":3}}"#,
        );
        let limited = interpret(&output(stream, ExitOutcome::Exited(1)), Role::Discovery)
            .expect_err("limit outcome");
        assert_eq!(
            limited.rate_limit().map(|(provider, _)| provider),
            Some("claude-code")
        );
        assert!(
            matches!(limited.usage(), Some(UsageValue::Reported { usage }) if usage.total() == 15)
        );
        assert!(limited.quota().is_some());

        let overloaded = interpret(
            &output(
                r#"{"type":"result","is_error":true,"result":"Overloaded (529)"}"#,
                ExitOutcome::Exited(1),
            ),
            Role::Discovery,
        )
        .expect_err("overload");
        assert!(overloaded.retryable());
        assert!(overloaded.rate_limit().is_none());
    }

    #[test]
    fn every_role_gets_the_isolation_flags_and_a_budget() {
        let runner = ClaudeRunner::default();
        for role in [
            Role::Discovery,
            Role::Planner,
            Role::Implementer,
            Role::Reviewer,
        ] {
            let args = runner.args(role);
            for flag in [
                "--restricted",
                "--strict-mcp-config",
                "--no-session-persistence",
            ] {
                assert!(args.iter().any(|a| a == flag), "{role:?} lacks {flag}");
            }
            let at = args
                .iter()
                .position(|a| a == "--max-budget-usd")
                .expect("budget");
            assert_eq!(args[at + 1], "3.00");
        }
    }

    #[test]
    fn read_only_roles_cannot_edit_or_run_commands() {
        let args = ClaudeRunner::default().args(Role::Reviewer);
        let at = args.iter().position(|a| a == "--tools").expect("tools");
        assert_eq!(args[at + 1], "Read,Grep,Glob");
        assert!(!args.iter().any(|a| a == "--permission-mode"));
        assert!(!args.iter().any(|a| a.starts_with("Bash")));
    }

    #[test]
    fn code_roles_may_edit_but_not_touch_history() {
        let args = ClaudeRunner::default().args(Role::Implementer);
        let at = args.iter().position(|a| a == "--tools").expect("tools");
        assert!(args[at + 1].contains("Edit") && args[at + 1].contains("Bash"));
        for sub in ["commit", "push", "tag", "reset"] {
            assert!(
                args.contains(&format!("Bash(git {sub} *)")),
                "git {sub} is not denied"
            );
        }
        assert!(!args.iter().any(|a| a == "--json-schema"));
    }

    #[test]
    fn document_roles_pass_their_schema() {
        let args = ClaudeRunner::default().args(Role::SpecWriter);
        let at = args
            .iter()
            .position(|a| a == "--json-schema")
            .expect("schema");
        let schema: serde_json::Value = serde_json::from_str(&args[at + 1]).expect("json");
        assert_eq!(schema["required"][0], "summary");
    }

    #[test]
    fn model_effort_and_budget_come_from_config() {
        let runner = ClaudeRunner::new(ClaudeConfig {
            model: Some("claude-sonnet-5-5".into()),
            effort: Some("high".into()),
            max_budget_usd: None,
            ..ClaudeConfig::default()
        });
        let args = runner.args(Role::Planner);
        assert!(
            args.windows(2)
                .any(|w| w == ["--model", "claude-sonnet-5-5"])
        );
        assert!(args.windows(2).any(|w| w == ["--effort", "high"]));
        assert!(!args.iter().any(|a| a == "--max-budget-usd"));
    }

    #[test]
    fn a_recorded_structured_run_becomes_a_reported_document() {
        let out = output(
            &fixture("02-minimal-schema-stream.ndjson"),
            ExitOutcome::Exited(0),
        );
        let result = interpret(&out, Role::Discovery).expect("ok");
        assert!(result.structured.is_some());
        assert!(matches!(result.usage, UsageValue::Reported { .. }));
    }

    #[test]
    fn recorded_git_denial_keeps_reported_usage_and_denial_details() {
        let out = output(
            &fixture("03-implementer-git-denied.ndjson"),
            ExitOutcome::Exited(0),
        );
        let result =
            interpret(&out, Role::Implementer).expect("denied command does not fail the whole run");
        assert!(matches!(result.usage, UsageValue::Reported { .. }));
        assert_eq!(result.permission_denials.len(), 1);
        assert_eq!(result.permission_denials[0].tool, "Bash");
        assert!(
            result
                .transcript
                .is_some_and(|text| text.contains("permission_denials"))
        );
    }

    #[test]
    fn a_run_stopped_by_its_spend_cap_fails_but_carries_its_real_usage() {
        let out = output(
            &fixture("04-max-budget-stop.ndjson"),
            ExitOutcome::Exited(1),
        );
        let err = interpret(&out, Role::SpecWriter).expect_err("the cap stopped the run");
        match err.usage() {
            Some(UsageValue::Reported { usage }) => assert_eq!(usage.total(), 6_186),
            other => panic!("the spend must be reported, got {other:?}"),
        }
    }

    #[test]
    fn a_recorded_run_that_loaded_connectors_fails_isolation() {
        let out = output(
            &fixture("01-readonly-stream.ndjson"),
            ExitOutcome::Exited(0),
        );
        match interpret(&out, Role::Reviewer) {
            Err(e) if !e.retryable() => assert!(e.to_string().contains("isolation failed"), "{e}"),
            other => panic!("expected an isolation failure, got {other:?}"),
        }
    }

    #[test]
    fn failures_are_classified_by_whether_retrying_can_help() {
        let result_line = |text: &str| {
            format!(
                r#"{{"type":"result","subtype":"error_during_execution","is_error":true,"result":"{text}","usage":{{"input_tokens":31,"output_tokens":9,"cache_read_input_tokens":4,"cache_creation_input_tokens":2}}}}"#
            )
        };
        let overloaded = output(
            &result_line("API Error: 529 overloaded"),
            ExitOutcome::Exited(1),
        );
        assert!(matches!(
            interpret(&overloaded, Role::Planner),
            Err(e) if e.retryable()
        ));
        let Err(overloaded_error) = interpret(&overloaded, Role::Planner) else {
            panic!("expected retryable failure")
        };
        assert!(
            matches!(overloaded_error.usage(), Some(UsageValue::Reported { usage }) if usage.input == 31 && usage.output == 9)
        );
        let budget = output(
            &result_line("Reached maximum budget ($3.00)"),
            ExitOutcome::Exited(1),
        );
        assert!(matches!(
            interpret(&budget, Role::Planner),
            Err(e) if !e.retryable()
        ));
        let silent = output("", ExitOutcome::Exited(1));
        assert!(matches!(
            interpret(&silent, Role::Planner),
            Err(e) if !e.retryable()
        ));
    }

    #[test]
    fn timeouts_truncation_and_signals_are_distinguished() {
        assert!(matches!(
            interpret(&output("", ExitOutcome::TimedOut), Role::Implementer),
            Err(e) if !e.retryable()
        ));
        assert!(matches!(
            interpret(&output("", ExitOutcome::Signaled), Role::Implementer),
            Err(e) if e.retryable()
        ));
        let mut big = output("", ExitOutcome::Exited(0));
        big.truncated = true;
        assert!(matches!(
            interpret(&big, Role::Implementer),
            Err(e) if !e.retryable()
        ));
    }

    #[test]
    fn json_in_the_result_text_is_accepted_when_there_is_no_structured_field() {
        let line =
            r#"{"type":"result","is_error":false,"result":"```json\n{\"summary\":\"x\"}\n```"}"#;
        let out = interpret(&output(line, ExitOutcome::Exited(0)), Role::Discovery).expect("ok");
        assert_eq!(out.structured.expect("doc")["summary"], "x");
        assert!(matches!(out.usage, UsageValue::Unavailable));
        let bad = r#"{"type":"result","is_error":false,"result":"here you go"}"#;
        assert!(matches!(
            interpret(&output(bad, ExitOutcome::Exited(0)), Role::Discovery),
            Err(AgentError::Retryable(_))
        ));
    }

    #[test]
    fn tail_never_splits_a_character() {
        let text = "é".repeat(500);
        assert!(tail(&text).chars().all(|c| c == 'é'));
    }
}
