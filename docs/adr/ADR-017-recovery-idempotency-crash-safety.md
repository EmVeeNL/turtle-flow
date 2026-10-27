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

## Implementation notes (2026-10-24)

- **Result and completion are atomic.** `Engine::finish_effect` marks the effect completed and applies the command it produced in **one transaction**. A crash can neither complete an effect without applying its result nor apply the result without completing the effect. Splitting these into two transactions is exactly the bug the crash harness catches.
- **Stale results are dropped, not failed.** If the job moved on (cancelled, paused, superseded) so that `decide()` rejects the produced command, the effect is completed with `command_rejected: <guard>` recorded.
- **Version conflicts retry the whole decision** on fresh state (up to 8 times), so a decision is never applied to a state it was not made for.
- **Startup recovery** (`Engine::recover`): instances that did not stop cleanly are marked stopped, their claimed effects return to `pending`, effects that already used every attempt become `failed` and their jobs are blocked. Each requeue and failure is an event, and `daemon.recovered` summarizes the run.
- **Crash injection.** `CrashPlan` marks every step boundary (after claim, after execution, around every decision and commit). The test harness crashes at boundary N, boots a fresh instance on the same on-disk database, and compares the finished job's event history and effect set with an uninterrupted run, for every N and for pairs of crashes. A separate seeded `kill -9` subprocess loop now exercises daemon restart through demo-agent delivery; it does not yet kill at every step boundary, and its kill timing is concentrated around scheduler and Git effects. Power-loss simulation remains planned.
- **Mutation-checked.** Replacing the atomic completion with two transactions fails the harness, and so does disabling recovery.
- **Not yet covered** (no code exists for them yet): orphaned agent processes, reservation and ledger reconciliation, recovery refs for interrupted agent work, and the effect-specific completion probes (worktree, commit, delivery). The generic protocol is proven; each executor must bring its own probe test when it is added.
## Executor probes (2026-10-25)

`FactoryExecutor` gives every effect kind its own completion probe, and the end-to-end crash test (`turtle-scheduler/tests/e2e.rs`) crashes at **every** "effect executed" boundary plus a spread of the others, with real Git:

| Effect | Probe | What a crash would otherwise cause |
|---|---|---|
| `create_worktree` | base commit recorded in the job first; worktree on the expected branch at that base; setup marker inside the Git directory | a second worktree, or setup skipped or repeated |
| `dispatch_ready_tasks` | commit with the task's `Turtle-Task: <plan revision>/<task>` trailer | the implementer is paid for twice |
| start run (spec, plan, review, verification) | a scheduler-authored artifact revision created after the effect | the agent is paid for twice |
| `run_gates` | none (harmless to repeat) | extra gate-result revision only |
| `request_approval` | identical pending approval | a second approval |
| `deliver` | commit with the job's `Turtle-Job` trailer on the target, or the parked `refs/turtle/delivery/<job>` | a second, empty squash commit |
| `cleanup_worktree` | removing a missing worktree succeeds | none |

The crash test asserts, for every sampled boundary: exactly one delivery commit, exactly one commit per task, the same artifact revisions as a clean run, the same stage transitions, **and exactly one agent call per role**. Removing the delivery probe, the task probe, or the artifact-reuse probe each fails it with a specific message.

Known gaps: a task whose agent run produced no change leaves no commit, so it is re-run after a crash; gate output beyond the last 4000 characters is not kept; `terminate_runs` cannot yet stop a run that is still executing (no run supervisor).
## Related ADRs

[ADR-004](ADR-004-sqlite-persistence-event-logging.md), [ADR-005](ADR-005-workflow-state-machine.md), [ADR-009](ADR-009-git-worktree-isolation.md), [ADR-011](ADR-011-job-scheduling-concurrency.md)
