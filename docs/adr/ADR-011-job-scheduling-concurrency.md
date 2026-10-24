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

## Related ADRs

[ADR-005](ADR-005-workflow-state-machine.md), [ADR-009](ADR-009-git-worktree-isolation.md), [ADR-017](ADR-017-recovery-idempotency-crash-safety.md), [ADR-015](ADR-015-future-server-deployment.md)
