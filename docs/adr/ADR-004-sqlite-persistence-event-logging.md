# ADR-004: SQLite Persistence and Event Logging

- **Status:** Accepted
- **Date:** 2026-10-24

## Context

Turtle Flow must never lose committed state (NFR-004), must provide a complete audit trail (FR-132, SEC-009), must support fast dashboard queries (NFR-016), and must run without external services. Phase 2 needs PostgreSQL.

## Problem Statement

Which persistence model provides durability, auditability and simplicity for a local single-writer system while remaining portable to a server database?

## Decision

1. **SQLite in WAL mode** as the only database in the MVP; `synchronous=FULL`, `foreign_keys=ON`; bundled SQLite with FTS5.
2. **State tables are authoritative** for current state (jobs, runs, approvals, budgets…), updated with optimistic concurrency (`version`).
3. An **append-only event log** (`events`) is written **in the same transaction** as every state change. Events are **hash-chained** and carry actor attribution.
4. An **effect outbox** (`effects`) is written in the same transaction; side effects execute after commit ([ADR-017](ADR-017-recovery-idempotency-crash-safety.md)).
5. **Single writer task** serializes all write transactions; reads use a read-only connection pool.
6. Large content lives in a **content-addressed blob store** on disk, indexed by SQLite ([ADR-013](ADR-013-resource-artifact-management.md)).
7. All SQL is confined to `turtle-store` behind repository traits.

## Considered Alternatives

| Alternative | Assessment |
|---|---|
| Full event sourcing (events as the only source of truth) | Strong audit and replay, but adds snapshotting, upcasting, projection lag and complexity; little benefit at this scale. Our hybrid gets most of the audit value |
| Embedded KV (redb, sled) | No SQL for analytics; more custom indexing code; sled maintenance concerns |
| PostgreSQL from day one | Requires a server process; contradicts local-first install simplicity |
| Files (JSON/YAML per job) | No transactions across entities; corruption risk; slow queries |
| DuckDB for analytics | Good OLAP, but second engine; SQLite suffices for local volumes; can export later |

## Rationale

SQLite is the most battle-tested embedded transactional store. Same-transaction events + state avoid dual-write inconsistencies. Hash chaining adds tamper evidence cheaply.

## Consequences

- One writer limits write throughput (adequate per A-07; benchmarked in S-7).
- Event payload schemas must be versioned; projections must be rebuildable (NFR-017) — tested.
- Database file can grow; retention and VACUUM policies needed ([DATA-MODEL.md §5](../DATA-MODEL.md#5-retention-and-size-management)).

## Risks

- Long write transactions (e.g. bulk FTS indexing) block other writes.
- `synchronous=FULL` latency on slow disks.

## Mitigations

- Chunk bulk work into small transactions; index extraction off the writer when possible (prepare outside, commit small).
- Measure; allow `NORMAL` for non-critical tables via separate attached DB if needed (e.g. transcripts index).

## Related ADRs

[ADR-013](ADR-013-resource-artifact-management.md), [ADR-016](ADR-016-observability-auditability.md), [ADR-017](ADR-017-recovery-idempotency-crash-safety.md), [ADR-015](ADR-015-future-server-deployment.md)
