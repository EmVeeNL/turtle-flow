# Turtle Flow — Architecture & Product Documentation Package

**Product name:** Turtle Flow
**CLI binary:** `turtle` (Cargo package `turtle-flow`; see OQ-07 in [PRD.md §26](PRD.md#26-open-questions))
**Repository:** `github.com/EmVeeNL/turtle-flow` (this package lives in `docs/`)
**Package version:** 0.1.0 (design baseline; implementation under way)
**Design date:** 2026-10-24 · **Last progress update:** 2026-10-25
**Status:** Design baseline, now being implemented: milestone M1 is complete and M2 is in progress (see [MVP.md §6.1](MVP.md#61-progress-against-the-milestones-2026-10-25)). Where code and these documents disagree, the amendment notes in the ADRs and §6.1 record why.

Turtle Flow is a local-first, CLI-first, Rust-based orchestrator that drives locally installed AI coding agents (initially Claude Code CLI and OpenAI Codex CLI) through a supervised, auditable software-delivery workflow: specification, human scope approval, planning, isolated implementation in Git worktrees, deterministic build/test gates, AI review, verification, human acceptance, and controlled delivery. A Rust core owns all state, policy, budgets, approvals and scheduling; AI agents are workers that produce artifacts.

---

## 1. How to read this package

| If you are… | Start with | Then read |
|---|---|---|
| Product / business stakeholder | [PRD.md](PRD.md) | [MVP.md](MVP.md), [LICENSING.md](LICENSING.md), [ROADMAP.md](ROADMAP.md) |
| Rust engineer starting implementation | [MVP.md](MVP.md) | [ARCHITECTURE.md](ARCHITECTURE.md), [DOMAIN-MODEL.md](DOMAIN-MODEL.md), [DATA-MODEL.md](DATA-MODEL.md), [WORKFLOW.md](WORKFLOW.md) |
| Agent / provider integrator | [AGENT-PROTOCOL.md](AGENT-PROTOCOL.md) | [TOKEN-INTELLIGENCE.md](TOKEN-INTELLIGENCE.md), [ADR-007](adr/ADR-007-provider-adapter-architecture.md), [ADR-018](adr/ADR-018-model-routing-and-evaluation.md) |
| Security reviewer | [SECURITY.md](SECURITY.md) | [ADR-012](adr/ADR-012-agent-execution-security.md), [ADR-006](adr/ADR-006-supervised-autonomy-approvals.md), [GIT-STRATEGY.md](GIT-STRATEGY.md) |
| Designer / frontend engineer | [DASHBOARD-UX.md](DASHBOARD-UX.md) | [images/README.md](images/README.md), [ADR-003](adr/ADR-003-embedded-webserver-dashboard.md) |
| QA / test engineer | [TESTING-STRATEGY.md](TESTING-STRATEGY.md) | [traceability/REQUIREMENTS-MATRIX.md](traceability/REQUIREMENTS-MATRIX.md) |
| Anyone wanting the critical view | [ARCHITECTURE-REVIEW.md](ARCHITECTURE-REVIEW.md) | [PRD.md §24–26](PRD.md#24-risks-and-dependencies) |

## 2. Package contents

```text
docs/
├── README.md                     ← this file: index, glossary, conventions
├── PRD.md                        ← product requirements (FR/NFR/SEC/UX IDs)
├── MVP.md                        ← MVP scope, epics, milestones, DoD
├── ARCHITECTURE.md               ← system & component architecture, crate layout
├── DOMAIN-MODEL.md               ← entities, invariants, lifecycle ownership
├── WORKFLOW.md                   ← state machine, guards, gates, retries, recovery
├── AGENT-PROTOCOL.md             ← provider adapter trait, contracts, CLI mapping
├── TOKEN-INTELLIGENCE.md         ← accounting, estimation, reservations, routing
├── SECURITY.md                   ← threat model, trust boundaries, controls
├── GIT-STRATEGY.md               ← worktrees, branches, commits, merges, recovery
├── LICENSING.md                  ← tiers, feature matrix, pricing hypotheses, enforcement
├── ROADMAP.md                    ← MVP → post-MVP → server → enterprise
├── DASHBOARD-UX.md               ← screens, journeys, visual design system
├── DATA-MODEL.md                 ← SQLite schema, indexes, ERD, migrations
├── TESTING-STRATEGY.md           ← test pyramid, harnesses, fixtures, security tests
├── ARCHITECTURE-REVIEW.md        ← critical review, top-10 risks, next steps
├── adr/                          ← 18 Architecture Decision Records + index
├── diagrams/                     ← standalone .mmd sources for all 12 diagrams + index
├── examples/                     ← config, workflow, job, agent request/response examples
├── images/                       ← 6 rendered dashboard mockups (PNG) + HTML sources + prompts
└── traceability/
    └── REQUIREMENTS-MATRIX.md    ← requirement → ADR → doc → MVP epic → test mapping
```

**Structure changes vs. the requested layout (with justification):**

1. **`ARCHITECTURE-REVIEW.md` added.** The critical review (top-10 risks, next steps) is a deliverable in its own right and is referenced by several documents; embedding it in `ARCHITECTURE.md` would mix "what we decided" with "what might go wrong".
2. **`images/src/` added.** The mockups were produced by rendering hand-authored HTML/CSS with headless Chrome — not by an AI image generator. The HTML sources are included so the mockups are reproducible, editable, and diffable. See [images/README.md](images/README.md).
3. **`examples/example-agent-response.ndjson` and `examples/example-preflight-report.json` added.** The agent protocol and the token preflight are the two contracts most likely to be misunderstood without a concrete payload.

## 3. Document status conventions

Every document marks statements using the following labels where the distinction matters:

| Label | Meaning |
|---|---|
| **Decision** | Confirmed architectural decision, recorded in an ADR with status *Accepted*. Changing it requires a superseding ADR. |
| **Proposal** | Recommended design choice; expected to hold, but may be revised during implementation without an ADR. |
| **Assumption** | Something we believe to be true but have not verified (e.g. provider CLI behavior). Each assumption has an ID `A-xx` in [PRD.md §25](PRD.md#25-assumptions). |
| **Future** | Considered for a post-MVP phase. Not to be implemented in the MVP, but the MVP must not preclude it. |
| **Open question** | Unresolved; has an ID `OQ-xx` in [PRD.md §26](PRD.md#26-open-questions). |
| **Uncertain** | Technical facts about third-party tools that could not be verified and must be confirmed by a spike before relying on them. |

Requirement identifiers: `FR-xxx` (functional), `NFR-xxx` (non-functional), `SEC-xxx` (security), `UX-xxx` (user experience), `LIC-xxx` (licensing). MVP priority uses MoSCoW: **Must**, **Should**, **Could**, **Won't (MVP)**.

## 4. Glossary (canonical terminology)

These terms are used with exactly these meanings across all documents. Full invariants are in [DOMAIN-MODEL.md](DOMAIN-MODEL.md).

| Term | Definition |
|---|---|
| **Installation** | One installation of Turtle Flow on one machine (MVP) or one server deployment (Phase 2). Owns the data directory `~/.turtle/` (configurable). |
| **Turtle Flow daemon** | The long-running process (`turtle daemon`) that hosts the scheduler, workflow engine, and embedded HTTP server. Exactly one per data directory. |
| **Project** | A unit of isolation bound to exactly one Git repository, with its own settings, budgets, policies, resources, knowledge, jobs and history. |
| **Job** | A unit of requested change to a project (feature, bugfix, refactor, docs). Has a workflow profile and moves through the job state machine. The unit a human approves and accepts. |
| **Workflow profile** | A named, versioned configuration (`fast`, `standard`, `strict`, or custom derived from these) that selects which stages, gates, approvals and limits apply to a job. |
| **Stage** | A named state in the job state machine (e.g. `specification`, `implementation`). |
| **Task** | A unit of work inside a job's approved plan (e.g. "add migration for table X"). Tasks form a DAG. Tasks are executed by one or more agent runs. |
| **Agent run** | One invocation of one provider CLI process for one purpose (role) with a bounded context package, budget reservation, and timeout. The atomic unit of AI execution and token accounting. |
| **Role** | The purpose of an agent run: `spec_writer`, `planner`, `implementer`, `fixer`, `reviewer`, `verifier`, `summarizer`, `release_notes`. |
| **Provider** | An AI agent product integrated via an adapter (e.g. `claude-code`, `codex`). |
| **Model** | A specific model offered through a provider (e.g. a Claude or GPT model identifier). |
| **Adapter** | Rust implementation of the `ProviderAdapter` trait that translates Turtle Flow requests into provider CLI invocations and normalizes output. |
| **Artifact** | An immutable, content-addressed, versioned output: specification, plan, patch/diff, review report, test report, verification report, release notes, approval evidence. Artifacts have revisions; each revision has a SHA-256. |
| **Resource** | A user-supplied input document (Markdown, PDF, OpenAPI, image, fixture…) attached to a project, job or task. Resources are inputs; artifacts are outputs. |
| **Approval** | A human decision (approve / reject / request changes) bound to a specific artifact revision hash, scope hash, policy version and budget envelope. Becomes `invalidated` when those inputs change materially. |
| **Quality gate** | A deterministic or AI-assisted check that must pass for a transition (e.g. `build`, `tests`, `lint`, `review_score`, `diff_size_limit`). Produces a gate result recorded as an artifact. |
| **Event** | An append-only record of something that happened (state transition, run started, tokens settled, approval granted). Events are the audit trail and the source of real-time UI updates. |
| **Delivery** | Integrating a job's accepted changes into the project's integration branch (merge, fast-forward, or pull request). Delivery makes code *available*; it does not deploy it. |
| **Release** | A separate entity that bundles one or more delivered jobs into a versioned, tagged, optionally deployed release with its own plan, approval, and rollback procedure. |
| **Budget** | A user-defined limit on token usage and/or monetary cost at Turtle Flow, project, job or task scope. |
| **Reservation** | Tokens/cost provisionally held against a budget before an agent run starts, settled to actuals when the run ends. |
| **Preflight** | The deterministic process run before a job (and before each agent run) that estimates complexity and usage, selects provider/model, computes reservations, and decides allow / require-approval / deny. |
| **Context package** | The exact, hashed bundle of instructions, artifacts, resource excerpts and knowledge items sent to an agent run. |
| **Worktree** | A Git worktree created per job for implementation isolation. Not a security boundary. |
| **Lease** | A time-bounded claim by the scheduler on a unit of work (agent run, gate execution) used for crash recovery. |

## 5. Key decisions at a glance

| # | Decision | ADR |
|---|---|---|
| 1 | Single-binary, local-first modular monolith (Cargo workspace), no microservices in MVP | [ADR-001](adr/ADR-001-local-first-modular-monolith.md) |
| 2 | Rust + Tokio + Clap + Axum + rusqlite/SQLite + Git CLI (git2/gix only for read paths) | [ADR-002](adr/ADR-002-rust-technology-stack.md) |
| 3 | Embedded Axum server serving a static Astro + Tailwind CSS v4 dashboard (Preact islands) via `rust-embed`; SSE for live updates; Asap Sharp + Fluent icons, self-hosted (amended 2026-10-25; was Vite + React + TanStack) | [ADR-003](adr/ADR-003-embedded-webserver-dashboard.md) |
| 4 | SQLite (WAL) with authoritative state tables plus an append-only, hash-chained event log written in the same transaction (not full event sourcing) | [ADR-004](adr/ADR-004-sqlite-persistence-event-logging.md) |
| 5 | Hand-written explicit state machine in Rust; profiles can only *select* from the built-in stage graph | [ADR-005](adr/ADR-005-workflow-state-machine.md) |
| 6 | Approvals bound to artifact hashes + scope + policy + budget envelope; automatic invalidation | [ADR-006](adr/ADR-006-supervised-autonomy-approvals.md) |
| 7 | `ProviderAdapter` trait + capability registry; headless CLI invocation with normalized event stream | [ADR-007](adr/ADR-007-provider-adapter-architecture.md) |
| 8 | Ledger-based token accounting with reservations; quantile-based historical estimation | [ADR-008](adr/ADR-008-token-intelligence-budgets.md) |
| 9 | One worktree + one branch per job; Turtle Flow (not agents) performs all commits and merges | [ADR-009](adr/ADR-009-git-worktree-isolation.md) |
| 10 | Retrieval-first project memory (structured facts + SQLite FTS5); embeddings and local LLM optional, post-MVP | [ADR-010](adr/ADR-010-project-memory-context-retrieval.md) |
| 11 | Single scheduler with DB-backed leases; per-project and per-provider concurrency limits | [ADR-011](adr/ADR-011-job-scheduling-concurrency.md) |
| 12 | Defense in depth: provider-native sandboxes + Turtle Flow process runner; OS/container isolation as opt-in; worktree ≠ sandbox | [ADR-012](adr/ADR-012-agent-execution-security.md) |
| 13 | Content-addressed blob store for resources and artifacts; metadata in SQLite | [ADR-013](adr/ADR-013-resource-artifact-management.md) |
| 14 | Offline-verifiable Ed25519-signed license files; no telemetry-based enforcement; Free tier needs no license | [ADR-014](adr/ADR-014-licensing-architecture.md) |
| 15 | Phase-2 server = same core crates + Postgres + worker nodes pulling leases over HTTPS | [ADR-015](adr/ADR-015-future-server-deployment.md) |
| 16 | Structured `tracing` logs, event log as audit trail, per-run transcripts stored as artifacts | [ADR-016](adr/ADR-016-observability-auditability.md) |
| 17 | Every side effect is a recorded, idempotent, resumable step; startup reconciliation | [ADR-017](adr/ADR-017-recovery-idempotency-crash-safety.md) |
| 18 | Rule-filter + weighted scoring router in MVP; offline evaluation suite; bandit routing later | [ADR-018](adr/ADR-018-model-routing-and-evaluation.md) |

## 6. What this package deliberately does *not* do

- It does not contain application code. Rust signatures and pseudocode are illustrative contracts.
- It does not claim that third-party CLIs behave in a specific way unless marked as verified; provider CLI flags and output formats change frequently and are listed as **Uncertain** with a required verification spike (see [AGENT-PROTOCOL.md §9](AGENT-PROTOCOL.md#9-provider-specifics-and-verification-spikes)).
- It does not present market-validated prices. All prices in [LICENSING.md](LICENSING.md) are hypotheses.
- It does not claim that the dashboard images are AI-generated renderings; they are HTML mockups rendered to PNG (see [images/README.md](images/README.md)).

## 7. Validation performed on this package

- All relative Markdown links and anchors were checked by script (see [ARCHITECTURE-REVIEW.md §5](ARCHITECTURE-REVIEW.md#5-package-validation-report)).
- All Mermaid diagrams were parsed with the Mermaid CLI where available (results in the same section).
- ADR decisions were cross-checked against PRD requirement priorities and MVP scope; the traceability matrix lists every `Must` requirement with its ADR, MVP epic and test reference.
