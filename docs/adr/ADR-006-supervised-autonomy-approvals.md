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

## Implementation notes (2026-10-24)

- **One builder for request and check.** `build_binding` creates the binding document when an approval is requested and again whenever a guard needs it. An approval is valid exactly when rebuilding the binding from the world as it is now gives the approved hash. Scope binds the specification revision; plan adds the plan; acceptance adds the verification report, the optional review report, and the Git state (base, head, diff hash, target branch).
- **Flags are part of the hash; acknowledgement is enforced at decision time.** The flags a human must see (protected paths, new dependencies) are computed from the diff and stored in the binding, so a changed set of flags changes the hash. `ApprovalService::decide` refuses an approval unless the caller acknowledges every flag in the binding. The guard therefore treats a valid acceptance approval as flags-acknowledged.
- **Three layers against stale approvals.** (1) The decision must echo the shown hash; (2) before recording, the service re-checks the pending approval against current state and invalidates it if the world moved (`Stale`); (3) the engine's guard recomputes validity again when applying the decision. Each layer has a mutation-checked test.
- **Eager invalidation.** `apply_decision` marks pending and approved approvals invalid in the same transaction when the workflow reports a material change (spec revised, base refreshed, …).
- **Fail closed.** If the decision context cannot read its facts, it returns an empty context: nothing is approved and nothing proceeds.
- **Not yet implemented:** the per-row HMAC from `docs/SECURITY.md` §6.5 layer L6 (it needs the OS keychain integration), approval expiry, and automatic scope approval for `fast` (`ContextConfig::auto_scope_enabled` exists but defaults to **off** until protected-path checks exist). Preflight is a placeholder that always allows, until the token engine lands (M3).
## Related ADRs

[ADR-005](ADR-005-workflow-state-machine.md), [ADR-008](ADR-008-token-intelligence-budgets.md), [ADR-009](ADR-009-git-worktree-isolation.md), [ADR-012](ADR-012-agent-execution-security.md), [ADR-016](ADR-016-observability-auditability.md)
