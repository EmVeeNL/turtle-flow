# ADR-007: AI Provider Adapter Architecture

- **Status:** Accepted
- **Date:** 2026-10-24

## Context

Turtle Flow must drive Claude Code CLI and OpenAI Codex CLI, and later other agents (FR-050 – FR-063). These CLIs differ in invocation, authentication, model selection, sessions, structured output, streaming formats, usage reporting, error reporting, cancellation, permissions/sandboxing and rate-limit signals — and they change frequently (R-01).

## Problem Statement

How do we integrate heterogeneous, fast-moving agent CLIs behind one stable interface without assuming features they may not have?

## Decision

1. A provider-neutral **`ProviderAdapter` trait** with: `discover`, `health`, pure `plan_invocation`, incremental `OutputParser`, `classify_exit`, `approx_tokens` ([AGENT-PROTOCOL.md §3](../AGENT-PROTOCOL.md#3-the-adapter-trait)).
2. Adapters are **translators only**. Process spawning (`turtle-exec`), supervision, limits, retries, routing, budgets and Git checks live outside adapters.
3. A **Capability Registry** stores discovered capabilities as `Known | Unsupported | Unknown`, versioned per CLI version; code must handle all three.
4. **Normalized `AgentEvent` stream** and `AgentRunResult` are the only outputs consumed by the rest of the system.
5. **Headless, one-process-per-run** invocation; prompts via stdin; optional session resume used only for schema repair.
6. Each adapter declares a **supported CLI version range**; outside it, runs are allowed only after a contract self-test with a warning.
7. Adapters are **compiled in** (no dynamic plugin loading in MVP).

## Considered Alternatives

| Alternative | Assessment |
|---|---|
| Call provider HTTP APIs directly and implement our own agent loop | Full control and precise usage, but re-implements tools, sandboxing and editing that the CLIs already do well; users want their CLI subscriptions used. Considered later for structured roles (OQ-05) |
| Use provider SDK "agent" libraries | Language/runtime mismatches (often TS/Python), still vendor-specific |
| MCP-based control of agents | MCP is for tools exposed *to* agents, not for orchestrating agent CLIs |
| Dynamic plugins (WASM/dylib) for adapters | Flexibility, but ABI/security complexity; premature |
| Interactive PTY scraping of TUIs | Fragile; headless modes exist |

## Rationale

Isolating volatility in small, golden-tested translators is the cheapest way to survive CLI churn. Pure invocation planning enables exhaustive tests and auditable commands.

## Consequences

- New CLI releases may require adapter releases; nightly canaries detect breakage early.
- Some features (mid-run token enforcement, native schemas) are available only on some providers; the system degrades explicitly.

## Risks

- A provider removes headless mode or changes licensing for automated use (OQ-01).
- Output formats change silently.

## Mitigations

- Multiple providers supported from day one; FakeAdapter keeps the system testable; direct-API adapters as fallback (Future).
- Strict parsers with unknown-event tolerance, recorded fixtures, contract suite, compat ranges.

## Related ADRs

[ADR-008](ADR-008-token-intelligence-budgets.md), [ADR-012](ADR-012-agent-execution-security.md), [ADR-018](ADR-018-model-routing-and-evaluation.md)
