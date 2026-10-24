# ADR-017: Recovery, Idempotency, and Crash Safety

- **Status:** Accepted
- **Date:** 2026-10-24

## Context

Jobs run for hours across machine sleep, crashes, CLI interruptions, rate limits, budget exhaustion and reboots (brief §3.5). Side effects (agent runs costing money, Git mutations, PRs) must not be duplicated or lost (NFR-004, NFR-005).

## Problem Statement

How do we guarantee that after any crash Turtle Flow converges to a correct state without duplicate side effects or manual repair?

## Decision

1. **Transactional outbox:** state change + events + effect intents commit atomically; effects execute only after commit ([ARCHITECTURE.md §3.2](../ARCHITECTURE.md#32-the-command--transaction--effect-loop)).
2. **Every effect has an idempotency key and a completion probe** that inspects the real world (Git refs, files, trailers, PR lists, transcripts) to detect prior completion before acting.
3. **Leases with instance IDs** on running work; a new daemon instance treats leases from dead instances as expired after verifying process liveness.
4. **Startup recovery** sequence (lock → integrity check → expire leases → orphan processes → transcript salvage → stash dirty worktrees → Git probes → re-dispatch effects → ledger reconciliation) before the scheduler starts ([WORKFLOW.md §10](../WORKFLOW.md#10-recovery-behavior)).
5. **Agent runs are not resumed mid-flight** after a daemon crash: if the transcript contains a complete result it is accepted; otherwise the run is `abandoned`, usage settled (reported or estimated), worktree changes stashed to a recovery ref, and the task retried.
6. **Crash-injection tests** for 25 defined step boundaries are part of the Definition of Done.

## Considered Alternatives

| Alternative | Assessment |
|---|---|
| Durable execution engine (Temporal/Restate) | Provides much of this, but heavy for local; also requires idempotent activities anyway |
| Best-effort recovery with manual repair | Unacceptable for unattended runs |
| Resume agent sessions after crash | Provider session resume semantics uncertain (S-5); partial state of tool executions unknown; clean retry is safer |
| Two-phase commit between DB and Git | Not possible; probes are the practical approach |

## Rationale

Outbox + probes is a proven pattern for exactly-once *effects* over at-least-once *execution*.

## Consequences

- Every effect needs careful probe design; more upfront work.
- Abandoned runs waste their tokens (recorded honestly).

## Risks

- Incorrect probe falsely concludes "done" or "not done".
- Stale lock removal racing with a live Git process.

## Mitigations

- Probes based on unique, Turtle Flow-generated markers (trailers with run IDs, specific ref names, PR head branches).
- Lock removal only after process-table check and age threshold; only in Turtle Flow worktrees.

## Related ADRs

[ADR-004](ADR-004-sqlite-persistence-event-logging.md), [ADR-005](ADR-005-workflow-state-machine.md), [ADR-009](ADR-009-git-worktree-isolation.md), [ADR-011](ADR-011-job-scheduling-concurrency.md)
