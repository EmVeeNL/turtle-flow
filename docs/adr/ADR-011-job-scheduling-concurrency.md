# ADR-011: Job Scheduling and Concurrency

- **Status:** Accepted
- **Date:** 2026-10-24

## Context

Multiple projects and jobs may be active (FR-037). Resources are limited by provider rate limits, local CPU (gates), and Git repository shared state. Concurrent work must not conflict and must survive crashes.

## Problem Statement

How does Turtle Flow decide what runs when, under which limits, and how is work claimed so that crashes do not cause duplicates or orphaned work?

## Decision

1. **Single scheduler** inside the daemon, triggered by events and a 2-second tick.
2. **Work units are effects** in the outbox; the dispatcher claims them with **leases** (`lease_instance_id`, `lease_expires_at`, renewed by heartbeat every 10 s, TTL 30 s).
3. **Concurrency limits** (configurable): global agent runs (default 3), per provider (2), per project jobs (2), gates (CPU/2), Git mutations per repository (1, serialized worker), one running agent run per job (J-2).
4. **Ordering:** priority → dependency readiness → age; approvals and holds never occupy slots.
5. **Conflict prediction** by expected/changed path overlap; policy `warn | serialize | allow`; actual conflicts via `merge-tree` probes.
6. **Adaptive provider concurrency** after rate-limit events ([TOKEN-INTELLIGENCE.md §14](../TOKEN-INTELLIGENCE.md#14-quota-and-rate-limit-handling)).
7. **Executor abstraction** between scheduler and supervisors (local in MVP; remote workers in Phase 2).

## Considered Alternatives

| Alternative | Assessment |
|---|---|
| External queue (Redis, NATS) | Unneeded locally; extra service |
| Per-job long-running tasks holding in-memory state | Loses state on crash; harder recovery |
| Unlimited concurrency | Triggers rate limits, CPU thrash, Git contention |
| Parallel tasks within one job (MVP) | Requires sub-worktrees and merges; deferred |

## Rationale

DB-backed leases unify scheduling and recovery; limits encode real bottlenecks; the executor abstraction is the seam for workers.

## Consequences

- Throughput bounded by conservative defaults; users can raise limits.
- Scheduler logic must be property-tested for limit invariants.

## Risks

- Starvation of low-priority jobs.
- Lease expiry during long blocking operations (e.g. system sleep) causing false orphan detection.

## Mitigations

- Aging boost (priority increases with wait time).
- On resume from sleep, supervisors renew leases before recovery evaluates them; orphan checks verify process liveness (PID + start time) before acting.

## Implementation notes (2026-10-24)

- **Claiming is one transaction** (`Tx::claim_effects`). It cancels pending non-control effects of blocked or finished jobs, skips jobs with holds, enforces per-job ordering (an effect waits for every earlier effect of the same job and for any claimed one, which gives J-2), skips effects in retry backoff, and then takes up to `max_concurrent` effects.
- **Control effects bypass holds and ordering:** `terminate_runs`, `release_reservations`, `supersede_pending_approvals`, `notify_user`, `cleanup_worktree` (`Effect::CONTROL_KINDS`). A hard pause must be able to stop a run that is still executing.
- **Leases are fenced by instance id.** Completion and failure require that the caller still holds the lease; otherwise the store returns `LeaseLost` and nothing is written. A hung old instance that finishes late cannot apply its result.
- **Heartbeats** renew the lease while an effect runs. Failure to renew stops the execution (the task is aborted) and the effect is treated as lost.
- **Executors run in their own task.** A panic in adapter code becomes a retryable failure instead of taking the dispatcher down (NFR-008).
- **Retry:** 10 s, 20 s, 40 s, 80 s, then 2 min, up to 5 attempts. A permanent failure (fatal error or attempts exhausted) blocks the job with `Unrecoverable{effect_failed:<kind>}`, in the same transaction.
- **Not yet implemented:** per-provider and per-project concurrency limits, priority ordering across jobs, and conflict prediction. The global limit is in place.
## Related ADRs

[ADR-005](ADR-005-workflow-state-machine.md), [ADR-009](ADR-009-git-worktree-isolation.md), [ADR-017](ADR-017-recovery-idempotency-crash-safety.md), [ADR-015](ADR-015-future-server-deployment.md)
