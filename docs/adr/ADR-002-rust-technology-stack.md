# ADR-002: Rust Technology Stack

- **Status:** Accepted (amended 2026-10-24: rusqlite replaces SQLx; see *Amendment* below)
- **Date:** 2026-10-24

## Context

The orchestrator must be reliable, efficient, and distributable as a single binary (NFR-001). It supervises many child processes, streams their output, serves HTTP/SSE, and persists state transactionally. The product vision explicitly favors Rust.

## Problem Statement

Which language-level libraries and frameworks should the Rust implementation standardize on?

## Decision

| Concern | Choice |
|---|---|
| Async runtime | **Tokio** (multi-threaded), `tokio::process` for children |
| CLI | **Clap** v4 derive API; completions & man pages generated |
| HTTP server | **Axum** + `tower-http` (compression, tracing, timeouts) |
| Database | **rusqlite** with bundled SQLite (see Amendment). ~~SQLx~~ |
| Serialization | `serde`, `serde_json`, `toml` |
| Schemas | `schemars` (generate), `jsonschema` (validate) |
| OpenAPI | `utoipa` |
| Templates | `minijinja` (prompt templates) |
| Errors | `thiserror` in libraries, `anyhow` only in the binary |
| Observability | `tracing`, `tracing-subscriber` (JSON), `tracing-appender` |
| IDs/time | `ulid`, `time` |
| Crypto | `sha2`, `hmac`, `ed25519-dalek` |
| OS integration | `nix` (process groups, rlimits, signals), `keyring`, `directories` |
| Git | **Git CLI** via `turtle-exec` for all mutations; optional `gix` for read-only fast paths (see ADR-009) |
| Testing | `proptest`, `insta`, `fail` (failpoints), `criterion`, `cargo-fuzz` |
| Supply chain | `cargo-deny`, `cargo-audit` |

MSRV: pinned to stable minus two releases at project start; no nightly features (NFR-015).

## Considered Alternatives

| Area | Alternative | Reason not chosen |
|---|---|---|
| Runtime | async-std / smol | Ecosystem centered on Tokio (Axum, SQLx, tower) |
| HTTP | Actix-web | Fine performance, but Axum integrates natively with Tokio/tower and has simpler extractor model |
| HTTP | Warp/Poem | Smaller ecosystems |
| DB | rusqlite (sync) | Simpler and very solid; would require `spawn_blocking` wrappers and lose compile-time checked queries. Reasonable fallback if SQLx SQLite limitations bite |
| DB | Diesel / SeaORM | ORM abstraction not needed; Diesel async story weaker; SeaORM adds layers |
| Git | git2 (libgit2) | C dependency, incomplete worktree/merge-tree parity with Git CLI; behavior differences vs. user's Git |
| Templates | Tera / Handlebars | MiniJinja is small, fast, Jinja-compatible |

## Rationale

Mainstream, well-maintained crates with large user bases reduce long-term risk. The Git CLI guarantees identical behavior to what users run themselves (config, LFS, credential helpers).

## Consequences

- Runtime dependency on Git ≥ 2.38.
- SQLx compile-time checks require committed `.sqlx` metadata in CI.
- Tokio everywhere: blocking work (PDF extraction, hashing large files) must use `spawn_blocking`.

## Risks

- SQLx SQLite driver limitations (e.g. connection-level PRAGMAs, busy handling) under single-writer design.
- Shelling out to Git has overhead and requires robust output parsing.

## Mitigations

- Spike S-7 benchmarks the store; rusqlite fallback behind the repository traits.
- Use porcelain v2 / `-z` machine-readable formats; typed wrapper with tests (TST-07).

## Amendment (2026-10-24): rusqlite instead of SQLx

**Decision:** the store uses **rusqlite** (bundled SQLite, FTS5 available) instead of SQLx. This is the fallback this ADR already named.

**Why, after building the store:**
- The design has exactly one writer doing short, multi-statement transactions with an optimistic version check. That fits a synchronous API; an async driver adds a pool, futures and cancellation cases around a single connection and gains nothing.
- Per-connection PRAGMAs (`synchronous=FULL`, `foreign_keys`, `busy_timeout`) and `VACUUM INTO` backups are direct calls in rusqlite.
- SQLx's compile-time query checking needs committed offline metadata and a prepared database in CI. We trade it for integration tests against a real in-memory database, which already caught two schema/code mismatches in the first PR (`loop_counters` default, `jobs.version` default).
- The async runtime still drives everything else. Store calls will run on a dedicated blocking task owned by the scheduler.

**Consequences:** SQL is not checked at compile time; every repository method needs a test. The Phase 2 PostgreSQL store (ADR-015) will use whatever driver suits it behind the same repository boundary.

## Related ADRs

[ADR-001](ADR-001-local-first-modular-monolith.md), [ADR-003](ADR-003-embedded-webserver-dashboard.md), [ADR-004](ADR-004-sqlite-persistence-event-logging.md), [ADR-009](ADR-009-git-worktree-isolation.md)
