# ADR-001: Local-First Modular Monolith

- **Status:** Accepted
- **Date:** 2026-10-24
- **Deciders:** Architecture team

## Context

Turtle Flow orchestrates long-running AI agent jobs on a developer's machine. The MVP targets individual developers (PRD §4) who want a tool they can install in minutes, that works offline except for AI provider calls (NFR-010), and that does not require an account or cloud service (LIC-001). A server product with multiple users and worker nodes is planned for Phase 2 ([ROADMAP.md §5](../ROADMAP.md#5-phase-2-turtle-flow-server-architecture)).

## Problem Statement

What deployment and code-organization style gives the MVP the lowest operational complexity and highest reliability while keeping a credible path to a multi-user server deployment?

## Decision

1. Build a **single executable** (`turtle`) containing the CLI, the daemon, the HTTP server and the embedded dashboard.
2. Organize the code as a **modular monolith**: a Cargo workspace of 13 crates with enforced dependency direction ([ARCHITECTURE.md §4.1](../ARCHITECTURE.md#41-crate-responsibilities-and-boundaries)).
3. Run a **long-lived daemon** per data directory (`turtle daemon`) that owns all state; CLI invocations are thin clients over a Unix socket.
4. Keep the domain, workflow and policy crates **free of IO and storage specifics** so they can be reused by the Phase 2 server.

## Considered Alternatives

| Alternative | Why not |
|---|---|
| Microservices (scheduler, API, executor as separate services) | Operational burden for a local tool, distributed failure modes, no benefit at single-user scale |
| CLI-only, no daemon (each command does its own work, state in DB) | Jobs outlive terminal sessions; dashboard needs a live server; multiple CLI processes would race as SQLite writers and schedulers |
| Desktop app (Tauri/Electron) | Heavier distribution, less natural for terminal-centric users; dashboard in browser suffices (A-08) |
| Cloud-hosted SaaS first | Contradicts local-first, privacy expectations; requires accounts, infrastructure, and trust in vendor with source code |

## Rationale

- One process owning state removes multi-writer coordination and makes crash recovery tractable (one place to reconcile).
- A modular monolith offers module boundaries without network boundaries; boundaries can become service boundaries later where justified (e.g. workers).
- A single binary keeps install and upgrade trivial.

## Consequences

- Positive: simple deployment, fast iteration, strong consistency, easy debugging.
- Negative: the daemon is a single point of failure on the machine (acceptable locally; mitigated by recovery); scaling is limited to one machine (by design in MVP).
- Requires a daemon lifecycle (start, stop, auto-start, single-instance lock).

## Risks

- Crate boundaries erode over time, making the server extraction expensive.
- Users find a background daemon surprising.

## Mitigations

- Dependency-direction checks in CI; repository traits; invariants I-1 – I-8 ([ARCHITECTURE.md §12](../ARCHITECTURE.md#12-architectural-constraints-and-invariants)).
- `turtle up/down/status`; daemon idle footprint target (NFR-006); auto-start configurable.

## Related ADRs

[ADR-002](ADR-002-rust-technology-stack.md), [ADR-003](ADR-003-embedded-webserver-dashboard.md), [ADR-004](ADR-004-sqlite-persistence-event-logging.md), [ADR-015](ADR-015-future-server-deployment.md), [ADR-017](ADR-017-recovery-idempotency-crash-safety.md)
