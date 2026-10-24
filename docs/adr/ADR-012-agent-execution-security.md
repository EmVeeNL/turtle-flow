# ADR-012: Agent Execution Security

- **Status:** Accepted
- **Date:** 2026-10-24

## Context

Agents execute shell commands and edit files based on prompts that may contain injected instructions (TH-01). Gates run project scripts (TH-03). In the MVP everything runs as the developer's OS user. Git worktrees provide change isolation, not security isolation.

## Problem Statement

What execution isolation and controls does the MVP require, and how do we evolve toward strong isolation without making the MVP impractical?

## Decision

1. **Layered isolation tiers** ([SECURITY.md §8](../SECURITY.md#8-isolation-tiers)):
   - Tier 0 (MVP, mandatory): provider-native sandbox/permission modes per role (read-only for analysis/review roles; workspace-write without network for implementers).
   - Tier 1 (MVP, mandatory): Turtle Flow runner — process groups, env allowlist, scratch TMPDIR, timeouts, output caps, rlimits, post-run Git/policy checks.
   - Tier 2 (post-MVP, opt-in; required for `untrusted` repos): rootless container for gates and agents.
   - Tier 3 (future): separate OS user / microVM.
2. **Bypass modes** of provider CLIs (skip permissions, full access) are disabled unless a `policy_exception` approval exists and Tier 2 is active.
3. **Least privilege per role** via `ToolPolicy`; restrictions that cannot be expressed are either errors or recorded best-effort gaps compensated by post-run checks.
4. **Implementer runs require** a provider with `sandbox_modes ⊇ {WorkspaceWrite}`; otherwise that provider cannot be routed to implementation.
5. Honest documentation of residual risks (SECURITY §12).

## Considered Alternatives

| Alternative | Assessment |
|---|---|
| Containers for everything in MVP | Strongest, but provider CLI auth inside containers, Docker availability on macOS, performance and DX are unresolved (spike S-4, OQ-03) |
| No sandbox, trust the agent | Unacceptable given prompt injection |
| Custom seccomp/Seatbelt profiles by Turtle Flow | High maintenance, platform-specific; providers already ship sandboxes; may complement later |
| Remote execution only (cloud sandboxes) | Contradicts local-first; Phase 2 option for workers |

## Rationale

Combining provider sandboxes with deterministic Turtle Flow checks gives meaningful protection now, while the tier model offers a clear upgrade path.

## Consequences

- Security level depends partly on provider sandbox quality (verified in S-4 and in TST-13 where feasible).
- Some legitimate agent actions (installing dependencies) fail without network; projects can allowlist registries if the sandbox supports it, or pre-install in `worktree_setup`.

## Risks

- Provider sandbox weaknesses or regressions.
- Users disable protections to "make it work".

## Mitigations

- Canary security tests per CLI release; `doctor` reports effective isolation.
- Bypass requires approval and is visible on every affected job and in the audit log.

## Related ADRs

[ADR-007](ADR-007-provider-adapter-architecture.md), [ADR-009](ADR-009-git-worktree-isolation.md), [ADR-006](ADR-006-supervised-autonomy-approvals.md), [ADR-016](ADR-016-observability-auditability.md)
