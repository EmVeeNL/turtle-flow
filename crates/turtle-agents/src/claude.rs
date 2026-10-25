//! Claude Code adapter pieces verified in spike S-1 (Claude Code 2.1.310).

use serde::Deserialize;
use serde_json::Value;
use turtle_domain::{QuotaStatus, QuotaWindow, TokenUsage, WindowStatus};

/// Flags every headless run must carry. Without them, the user's claude.ai connectors,
/// skills and slash commands load into the run and inflate cost ~45× (spike S-1 §1.3).
pub const ISOLATION_FLAGS: &[&str] = &[
    "--print",
    "--output-format",
    "stream-json",
    "--verbose",
    "--permission-prompts",
    "none",
    "--restricted",
    "--strict-mcp-config",
    "--mcp-config",
    r#"{"mcpServers":{}}"#,
    "--setting-sources",
    "",
    "--disable-slash-commands",
    "--exclude-dynamic-system-prompt-sections",
    "--no-session-persistence",
];

/// Tools that may appear in `system/init` for an isolated run; anything else is a leak.
pub const EXPECTED_TOOLS: &[&str] = &[
    "Read",
    "Grep",
    "Glob",
    "Edit",
    "Write",
    "Bash",
    "StructuredOutput",
];

/// A tool call the CLI refused because of permissions (e.g. a denied `git commit`).
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct PermissionDenial {
    pub tool: String,
    pub input: Value,
}

/// Summary of one Claude Code stream-json run, built incrementally from stdout lines.
#[derive(Clone, Debug, Default)]
pub struct RunSummary {
    pub session_id: Option<String>,
    pub cli_version: Option<String>,
    pub model: Option<String>,
    pub tools: Vec<String>,
    pub mcp_servers: Vec<String>,
    pub usage: Option<TokenUsage>,
    pub cost_reported_usd: Option<f64>,
    pub result_text: Option<String>,
    pub structured_output: Option<Value>,
    pub permission_denials: Vec<PermissionDenial>,
    pub quota: Option<QuotaStatus>,
    pub is_error: Option<bool>,
    pub num_turns: Option<u32>,
    /// Lines that were not valid JSON or had an unknown `type` (tolerated, counted).
    pub unknown_lines: usize,
}

impl RunSummary {
    /// Tools or MCP servers present that an isolated run must not have.
    #[must_use]
    pub fn isolation_leaks(&self) -> Vec<String> {
        let mut leaks: Vec<String> = self
            .tools
            .iter()
            .filter(|t| !EXPECTED_TOOLS.contains(&t.as_str()))
            .cloned()
            .collect();
        leaks.extend(self.mcp_servers.iter().map(|s| format!("mcp:{s}")));
        leaks
    }
}

#[derive(Deserialize)]
struct Usage {
    #[serde(default)]
    input_tokens: u64,
    cache_creation_input_tokens: Option<u64>,
    cache_read_input_tokens: Option<u64>,
    #[serde(default)]
    output_tokens: u64,
    output_tokens_details: Option<OutputDetails>,
}

#[derive(Deserialize)]
struct OutputDetails {
    thinking_tokens: Option<u64>,
}

/// Incremental parser for `--output-format stream-json` output.
#[derive(Debug, Default)]
pub struct StreamParser {
    summary: RunSummary,
}

impl StreamParser {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn on_line(&mut self, line: &str) {
        let line = line.trim();
        if line.is_empty() {
            return;
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            self.summary.unknown_lines += 1;
            return;
        };
        let s = &mut self.summary;
        match (v["type"].as_str(), v["subtype"].as_str()) {
            (Some("system"), Some("init")) => {
                s.session_id = v["session_id"].as_str().map(Into::into);
                s.cli_version = v["claude_code_version"].as_str().map(Into::into);
                s.model = v["model"].as_str().map(Into::into);
                s.tools = string_list(&v["tools"]);
                s.mcp_servers = v["mcp_servers"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|m| m["name"].as_str().map(Into::into))
                            .collect()
                    })
                    .unwrap_or_default();
            }
            (Some("rate_limit_event"), _) => {
                s.quota = parse_rate_limit(&v["rate_limit_info"]);
            }
            (Some("result"), _) => {
                s.is_error = v["is_error"].as_bool();
                s.num_turns = v["num_turns"].as_u64().and_then(|n| u32::try_from(n).ok());
                s.cost_reported_usd = v["total_cost_usd"].as_f64();
                s.result_text = v["result"].as_str().map(Into::into);
                s.structured_output = v.get("structured_output").filter(|o| !o.is_null()).cloned();
                if let Ok(u) = serde_json::from_value::<Usage>(v["usage"].clone()) {
                    s.usage = Some(TokenUsage {
                        input: u.input_tokens,
                        cached_input_read: u.cache_read_input_tokens,
                        cached_input_write: u.cache_creation_input_tokens,
                        output: u.output_tokens,
                        reasoning: u
                            .output_tokens_details
                            .and_then(|d| d.thinking_tokens)
                            .filter(|&t| t > 0),
                    });
                }
                s.permission_denials = v["permission_denials"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .map(|d| PermissionDenial {
                                tool: d["tool_name"].as_str().unwrap_or_default().to_owned(),
                                input: d["tool_input"].clone(),
                            })
                            .collect()
                    })
                    .unwrap_or_default();
            }
            (Some("system" | "assistant" | "user"), _) => {}
            _ => s.unknown_lines += 1,
        }
    }

    #[must_use]
    pub fn finish(self) -> RunSummary {
        self.summary
    }
}

fn string_list(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|t| t.as_str().map(Into::into))
                .collect()
        })
        .unwrap_or_default()
}

fn parse_rate_limit(info: &Value) -> Option<QuotaStatus> {
    let windows = info["unifiedWindows"].as_object()?;
    let mut out: Vec<QuotaWindow> = windows
        .iter()
        .filter_map(|(name, w)| {
            Some(QuotaWindow {
                name: name.clone(),
                utilization: w["utilization"].as_f64()?,
                resets_at: w["resetsAt"].as_i64(),
                status: match w["status"].as_str() {
                    Some("allowed") => WindowStatus::Allowed,
                    Some("allowed_warning") => WindowStatus::Warning,
                    _ => WindowStatus::Rejected,
                },
            })
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Some(QuotaStatus::Partial { windows: out })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> RunSummary {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../spikes/fixtures/claude-code/"
        );
        let text = std::fs::read_to_string(format!("{path}{name}")).expect("fixture exists");
        let mut p = StreamParser::new();
        text.lines().for_each(|l| p.on_line(l));
        p.finish()
    }

    #[test]
    fn non_isolated_run_shows_leaks_and_cost() {
        let s = fixture("01-readonly-stream.ndjson");
        assert_eq!(s.cli_version.as_deref(), Some("2.1.310"));
        assert_eq!(s.mcp_servers.len(), 6);
        assert!(!s.isolation_leaks().is_empty());
        let usage = s.usage.expect("usage reported");
        assert_eq!(usage.cached_input_write, Some(223_442));
        assert!(s.cost_reported_usd.expect("cost") > 0.5);
        assert_eq!(s.unknown_lines, 0);
    }

    #[test]
    fn isolated_run_has_structured_output_and_no_leaks() {
        let s = fixture("02-minimal-schema-stream.ndjson");
        assert!(s.isolation_leaks().is_empty(), "{:?}", s.isolation_leaks());
        assert_eq!(
            s.structured_output,
            Some(serde_json::json!({"printed": "hello"}))
        );
        assert!(s.cost_reported_usd.expect("cost") < 0.05);
        assert_eq!(s.is_error, Some(false));
    }

    #[test]
    fn denied_git_commit_is_reported() {
        let s = fixture("03-implementer-git-denied.ndjson");
        assert_eq!(s.permission_denials.len(), 1);
        let d = &s.permission_denials[0];
        assert_eq!(d.tool, "Bash");
        assert!(
            d.input["command"]
                .as_str()
                .expect("command")
                .starts_with("git commit")
        );
    }

    #[test]
    fn quota_windows_are_parsed() {
        let s = fixture("01-readonly-stream.ndjson");
        let Some(QuotaStatus::Partial { windows }) = s.quota else {
            panic!("expected partial quota")
        };
        let names: Vec<_> = windows.iter().map(|w| w.name.as_str()).collect();
        assert_eq!(names, ["five_hour", "seven_day"]);
        assert_eq!(windows[1].status, WindowStatus::Warning);
    }
}
