# ADR-006: Supervised Autonomous Approval Model

- **Status:** Accepted
- **Date:** 2026-10-24

## Context

The chosen autonomy model is *Supervised Autonomous*: agents act independently inside approved scope and budget; humans approve scope, significant changes, budget increases, protected-branch integration and releases (brief §3.3). Approvals must bind to exact artifact revisions and become invalid when assumptions change.

## Problem Statement

How do we represent approvals so that (a) humans approve exactly what they saw, (b) approvals cannot be spoofed or reused after material changes, and (c) autonomy can be safely relaxed for low-risk work?

## Decision

1. An **Approval** is an entity bound to a canonical **binding document** (artifact revision hashes, base/head commits, diff hash, profile hash, policy version, budget envelope, acknowledged flags). `subject_hash = SHA-256(JCS(binding))` ([WORKFLOW.md §6.2](../WORKFLOW.md#62-approval-binding)).
2. Decisions must **echo `subject_hash`**; mismatch rejects the request.
3. **Validity is recomputed at every guard** that depends on the approval, in addition to eager invalidation events.
4. **Gate kinds:** `scope`, `plan`, `budget_increase`, `scope_change`, `policy_exception`, `acceptance`, `release`.
5. **Acceptance is mandatory** for delivery to protected branches in every profile; **release approval** is mandatory always. Only `scope` can be auto-approved, only in `fast`, under explicit rules, recorded with rule ID.
6. **Channels:** authenticated dashboard or interactive CLI TTY only; agent output is never interpreted as approval. Approval rows carry an **HMAC** keyed from the OS keychain.

## Considered Alternatives

| Alternative | Assessment |
|---|---|
| Boolean `approved` flags on jobs | Cannot express *what* was approved; trivially stale |
| Approve-by-stage (any revision) | Allows bait-and-switch after approval |
| Signatures with user keys (GPG/SSH) on each approval | Strong, but heavy UX for local single user; reconsider for Enterprise four-eyes policies |
| Full autonomy with post-hoc review | Contradicts chosen model; unacceptable for protected branches |

## Rationale

Binding approvals to hashes is cheap and makes stale approvals impossible by construction. Recomputing at guard time defends against missed invalidations.

## Consequences

- Any change after approval (even a whitespace fix) requires re-approval of acceptance — intended; UI must make re-approval fast (show delta since last approved revision).
- Canonical JSON and diff normalization must be stable across versions (versioned binding schema).

## Risks

- Approval fatigue leading to rubber-stamping.
- Same-user processes forging approvals ([SECURITY.md §6.5](../SECURITY.md#65-approval-bypass-prevention)).

## Mitigations

- Show deltas, flags and invalidation reasons prominently; batch low-risk scope approvals; `fast` auto-scope rules.
- Layered bypass defenses L1–L8; HMAC + audit chain; container isolation option.

## Related ADRs

[ADR-005](ADR-005-workflow-state-machine.md), [ADR-008](ADR-008-token-intelligence-budgets.md), [ADR-009](ADR-009-git-worktree-isolation.md), [ADR-012](ADR-012-agent-execution-security.md), [ADR-016](ADR-016-observability-auditability.md)
