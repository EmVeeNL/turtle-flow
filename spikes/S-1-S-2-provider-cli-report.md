# Spike S-1 / S-2 — Provider CLI Headless Contracts (first pass)

| Field | Value |
|---|---|
| Date | 2026-10-24 |
| Machine | macOS (arm64), Git 2.54.0, Rust 1.98.1 |
| Claude Code | 2.1.310 |
| Codex CLI | 0.163.0 |
| Raw material | [`raw/`](raw/) (`--help` captures), [`fixtures/`](fixtures/) (recorded headless transcripts) |
| Status | S-1 partially complete (3 runs). S-2 blocked by account usage limit; only the error path is recorded. |

Fixtures were recorded in a throwaway Git repository containing `main.rs` and `README.md`. Session IDs and local paths are left in on purpose because the parsers must handle them. No secrets are present.

## 1. Claude Code (S-1)

### 1.1 Verified flags (from `claude --help` and real runs)

| Concern | Verified mechanism | Doc impact |
|---|---|---|
| Headless | `-p/--print`, prompt via stdin ✔ | as designed |
| Streaming | `--output-format stream-json` + `--verbose` ✔ | as designed |
| **Structured output** | **`--json-schema '<schema>'` ✔.** The result event carries a parsed `structured_output` object, and `result` holds the JSON string. A `StructuredOutput` tool is added automatically. | Resolves the "Uncertain" in AGENT-PROTOCOL §9.1, so the native schema path is available |
| Turn limit | **No `--max-turns` flag** in 2.1.310 | Wall-clock and budget limits only |
| **Budget cap** | **`--max-budget-usd <amount>`** (print mode) | New native hard-stop. Map run reservations to it in API-equivalent USD |
| Tool set | `--tools Read,Edit,Bash` limits the available built-in tools; `--allowedTools` / `--disallowedTools` take patterns like `Bash(git commit *)` | as designed |
| **Headless permission handling** | **`--permission-prompts none`**: anything that would prompt is denied automatically | Required for unattended runs |
| **Restricted mode** | **`--restricted`** removes code-running tools unless `--tools` names them, ignores user/project/local settings, confines file tools to the working dirs, refuses `bypassPermissions`, and blocks agent writes to settings, git and tool-config files | Strong default for every role; the implementer adds `Bash` explicitly |
| Permission modes | `acceptEdits`, `auto`, `bypassPermissions`, `manual`, `dontAsk`, `plan` | `plan` was not needed once `--restricted --tools Read` was used |
| Config isolation | `--strict-mcp-config --mcp-config '{"mcpServers":{}}'`, `--setting-sources ""`, `--disable-slash-commands`, `--exclude-dynamic-system-prompt-sections` ✔ | See §1.3 (cost) |
| Sessions | `--session-id <uuid>`, `--resume`, `--fork-session`, `--no-session-persistence` | Use `--no-session-persistence` unless a repair resume is planned |
| `--bare` | Skips hooks, CLAUDE.md, keychain and OAuth, so auth must be an API key | Not usable with subscription auth. Option for API-key mode only |
| Effort | `--effort low…max` | New routing dimension (model × effort) |
| Fallback | `--fallback-model a,b` | Prefer factory-side routing; leave unset so usage attribution stays exact |

### 1.2 Stream event types observed

`system/init`, `system/commands_changed`, `assistant`, `user` (tool results), **`rate_limit_event`**, `result/success`.

`system/init` reports the effective `tools`, `mcp_servers`, `skills`, `plugins`, `agents`, `model`, `permissionMode`, `claude_code_version` and `apiKeySource`. That is exactly what capability discovery and the run audit need. Persist it in the transcript and assert on it: for example, fail the run if unexpected MCP servers are loaded.

The `result` event includes `usage` (input, cache creation split into 5m/1h, cache read, output, thinking), a per-model `modelUsage` with `costUSD`, `contextWindow` and `maxOutputTokens`, plus `total_cost_usd`, `num_turns`, `permission_denials[]`, `stop_reason`, `terminal_reason`, `structured_output` and `duration_ms`.

### 1.3 Finding: inherited user configuration dominates cost

| Run | Config | Tools loaded | Cache-creation tokens | Cost (API-equivalent) |
|---|---|---|---|---|
| 01 read-only | `--restricted --tools Read,Grep,Glob` only | 11 (incl. 8 claude.ai connector MCP tools), 6 MCP servers, 17 skills, 53 slash commands | 223,442 | **$0.897** |
| 02 read-only + schema | + `--strict-mcp-config`, empty MCP config, `--setting-sources ""`, `--disable-slash-commands`, `--exclude-dynamic-system-prompt-sections` | 2 (`Read`, `StructuredOutput`), 0 MCP, 0 skills | 3,761 | **$0.020** |
| 03 implementer | same isolation, `Read,Edit,Bash` | 3 | small | **$0.020** |

**A ~45× difference for the same task.** `--restricted` alone does not drop claude.ai connectors, plugins or skills. Every agent run must use the full isolation flag set, and the adapter should assert from `system/init` that no unexpected tools or MCP servers loaded. This matters more for token cost than any estimation work. Three plugins and five agents still loaded even in isolated mode; check whether `--plugin-dir` with an empty directory or a dedicated `CLAUDE_CONFIG_DIR` removes them (OQ-03).

### 1.4 Finding: subscription quota is visible

`rate_limit_event.rate_limit_info` reports:

```json
{"status":"allowed_warning","rateLimitType":"seven_day","utilization":0.76,"resetsAt":1793109600,
 "surpassedThreshold":0.75,"overageStatus":"rejected","isUsingOverage":false,
 "unifiedWindows":{"five_hour":{"utilization":0.65,"resetsAt":1792848600,"status":"allowed"},
                   "seven_day":{"utilization":0.76,"resetsAt":1793109600,"status":"allowed_warning"}}}
```

Assumption **A-03 is false for Claude Code with subscription auth.** Window utilization (a fraction, not tokens) and reset times are available on every run. Implications:
- `quota_visibility` for `claude-code` = `Partial` (utilization fraction and reset per window; no absolute token counts).
- The scheduler can avoid starting long runs when the 5-hour window is close to its limit, and the dashboard can show real utilization bars labelled "% of provider window".
- Persist each observation in `provider_quota_snapshots`.

### 1.5 Finding: permission enforcement

- `Bash(git commit *)` in `--disallowedTools` was **denied** and recorded in `result.permission_denials[]` with tool name and input. Emit these as `policy_denied` events.
- `find …`, which was not in `--allowedTools`, **ran anyway.** Read-only shell commands appear to be auto-approved under `acceptEdits`. The allowlist is therefore not a complete restriction for read commands, and the post-run Git policy check (FR-093) stays mandatory.
- `ls` reported "completed with no output" although files existed. Unexplained; re-test before relying on shell output in fixtures.
- The model's commit message included a `Co-Authored-By` trailer. The agent tries to commit when asked, so the factory-side prompt must say plainly "never commit".

## 2. Codex CLI (S-2)

### 2.1 Verified flags (from `codex exec --help`)

| Concern | Mechanism | Doc impact |
|---|---|---|
| Headless | `codex exec`, prompt via stdin with `-` ✔ | as designed |
| Streaming | `--json` → JSONL ✔ (`thread.started`, `turn.started`, `error`, `turn.failed` observed) | as designed |
| Structured output | `--output-schema <FILE>` (flag present; success path not yet observed) | as designed |
| Sandbox | `-s read-only \| workspace-write \| danger-full-access` | as designed |
| Approvals | **No `-a/--ask-for-approval` on `exec`.** `--approve-for-me` routes approvals to automatic review | Remove `-a never` from the docs |
| Config isolation | `--ignore-user-config` (auth still read from `CODEX_HOME`), `--ignore-rules`, `--strict-config` | Use `--ignore-user-config` by default, the Codex equivalent of §1.3 |
| Persistence | `--ephemeral` | Default for agent runs |
| Working dir | `-C/--cd`, `--add-dir`, `--skip-git-repo-check` | as designed |
| Sessions | `exec resume <id>`, `exec fork <id>` | as designed |
| Built-in review | `exec review` | Possible reviewer-role shortcut; evaluate later |
| Local models | `--oss --local-provider ollama\|lmstudio` | Route to local models later through the same adapter |

### 2.2 Rate-limit error format

```json
{"type":"error","message":"You’ve hit your usage limit. Upgrade to Pro (...), visit ... or try again at 3:36 PM."}
{"type":"turn.failed","error":{"message":"...same..."}}
```

- There is no structured retry field. The retry time is a **local wall-clock string** ("3:36 PM") with no date or timezone. Parse it as the next occurrence in local time and fall back to the backoff policy if parsing fails.
- Exit code was 1. Classify as `RateLimited` from the message pattern `hit your usage limit`.

### 2.3 Success path (recorded 2026-10-25, Codex 0.163.0, model `gpt-6-luna`, effort low)

Fixtures: `fixtures/codex/02-readonly-structured.ndjson` (read-only, `--output-schema`, schema in `02-discovery-schema.json`) and `03-implementer-commit-attempt.ndjson` (workspace-write; the prompt asks the agent to commit). Invocation used: `codex exec --json --ephemeral --ignore-user-config -m <model> -c model_reasoning_effort=low -s <read-only|workspace-write> [--output-schema FILE] -C <dir> -` with the prompt on stdin.

- **Event stream:** `thread.started`, `turn.started`, `item.started`/`item.completed` (types `agent_message` and `command_execution` with `command`, `aggregated_output`, `exit_code`, `status`), `turn.completed`. File edits made through Codex's patch tool did **not** appear as items in run 03: the Git post-run snapshot, not the stream, is the source of truth for what changed.
- **Structured output** has no separate field: the final `agent_message` `text` is the JSON document (valid against the schema). Take the last `agent_message` of the turn.
- **Usage** is in `turn.completed.usage`: `input_tokens`, `cached_input_tokens`, `cache_write_input_tokens`, `output_tokens`, `reasoning_output_tokens`. In run 02, `input_tokens` 52,515 with `cached_input_tokens` 34,432: **`input_tokens` includes the cached reads** (OpenAI convention), unlike Claude Code, whose `input_tokens` excludes them. Adding the two would double count. Reasoning tokens are part of `output_tokens`.
- **Sandbox:** in `workspace-write` the agent could create `HELLO.md` but `git add` failed (`Git could not create .git/index.lock`, exit 128): the Codex sandbox protects `.git`. The agent reported this honestly. Defense in depth for "agents never commit", though the factory-side policy check still decides.
- Auth came from `CODEX_HOME` even with `--ignore-user-config`; no `--approve-for-me` was needed for these runs.

## 3. Required documentation updates

Applied in this commit: AGENT-PROTOCOL §9 (verified facts), PRD A-03 (falsified for Claude Code), TOKEN-INTELLIGENCE §14 (quota visibility).

## 4. Next spike runs (when Codex quota resets)

1. Codex read-only + `--output-schema` success path (usage fields on `turn.completed`).
2. Codex `workspace-write` implementer: file edits, attempted `git commit`, network attempt.
3. Claude: cancellation (SIGTERM mid-run). Does the stream end with a `result`? Is partial usage reported?
4. Claude: auth-missing error shape (run with an empty `CLAUDE_CONFIG_DIR`).
5. S-4 sandbox: can an implementer read `~/.turtle`, write outside the worktree, or reach `127.0.0.1`?
