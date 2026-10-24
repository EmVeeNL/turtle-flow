# Architecture Decision Records

ADRs record significant architectural decisions with context, alternatives and consequences. Format: Title, Status, Context, Problem Statement, Decision, Considered Alternatives, Rationale, Consequences, Risks, Mitigations, Related ADRs.

**Status values:** *Proposed* → *Accepted* → (*Deprecated* | *Superseded by ADR-xxx*). Changing an accepted decision requires a new ADR that supersedes it.

| ADR | Title | Status | Primary documents |
|---|---|---|---|
| [ADR-001](ADR-001-local-first-modular-monolith.md) | Local-First Modular Monolith | Accepted | [ARCHITECTURE.md](../ARCHITECTURE.md) |
| [ADR-002](ADR-002-rust-technology-stack.md) | Rust Technology Stack | Accepted | [ARCHITECTURE.md §9](../ARCHITECTURE.md#9-technology-choices-summary) |
| [ADR-003](ADR-003-embedded-webserver-dashboard.md) | Embedded Webserver and Dashboard | Accepted | [DASHBOARD-UX.md](../DASHBOARD-UX.md) |
| [ADR-004](ADR-004-sqlite-persistence-event-logging.md) | SQLite Persistence and Event Logging | Accepted | [DATA-MODEL.md](../DATA-MODEL.md) |
| [ADR-005](ADR-005-workflow-state-machine.md) | Workflow State Machine | Accepted | [WORKFLOW.md](../WORKFLOW.md) |
| [ADR-006](ADR-006-supervised-autonomy-approvals.md) | Supervised Autonomous Approval Model | Accepted | [WORKFLOW.md §6](../WORKFLOW.md#6-approvals) |
| [ADR-007](ADR-007-provider-adapter-architecture.md) | AI Provider Adapter Architecture | Accepted | [AGENT-PROTOCOL.md](../AGENT-PROTOCOL.md) |
| [ADR-008](ADR-008-token-intelligence-budgets.md) | Token Intelligence and Budget Management | Accepted | [TOKEN-INTELLIGENCE.md](../TOKEN-INTELLIGENCE.md) |
| [ADR-009](ADR-009-git-worktree-isolation.md) | Git Worktree Isolation | Accepted | [GIT-STRATEGY.md](../GIT-STRATEGY.md) |
| [ADR-010](ADR-010-project-memory-context-retrieval.md) | Project Memory and Context Retrieval | Accepted | [TOKEN-INTELLIGENCE.md §10](../TOKEN-INTELLIGENCE.md#10-context-size-optimization) |
| [ADR-011](ADR-011-job-scheduling-concurrency.md) | Job Scheduling and Concurrency | Accepted | [WORKFLOW.md §11](../WORKFLOW.md#11-job-dependencies-and-parallel-execution) |
| [ADR-012](ADR-012-agent-execution-security.md) | Agent Execution Security | Accepted | [SECURITY.md](../SECURITY.md) |
| [ADR-013](ADR-013-resource-artifact-management.md) | Resource and Artifact Management | Accepted | [DATA-MODEL.md §3.6](../DATA-MODEL.md#36-resources-knowledge-search) |
| [ADR-014](ADR-014-licensing-architecture.md) | Licensing Architecture | Accepted | [LICENSING.md](../LICENSING.md) |
| [ADR-015](ADR-015-future-server-deployment.md) | Future Server Deployment | Accepted (direction) | [ROADMAP.md §5](../ROADMAP.md#5-phase-2-turtle-flow-server-architecture) |
| [ADR-016](ADR-016-observability-auditability.md) | Observability and Auditability | Accepted | [SECURITY.md §6.7](../SECURITY.md#67-audit-log-sec-009) |
| [ADR-017](ADR-017-recovery-idempotency-crash-safety.md) | Recovery, Idempotency, and Crash Safety | Accepted | [WORKFLOW.md §10](../WORKFLOW.md#10-recovery-behavior) |
| [ADR-018](ADR-018-model-routing-and-evaluation.md) | Model Routing and Evaluation | Accepted | [AGENT-PROTOCOL.md §11](../AGENT-PROTOCOL.md#11-evaluation-framework-summary) |

## Decision dependency overview

```text
ADR-001 monolith ─┬─ ADR-002 stack ─── ADR-003 dashboard
                  ├─ ADR-004 SQLite/events ─── ADR-017 recovery ─── ADR-011 scheduling
                  └─ ADR-015 server (future) ── ADR-014 licensing
ADR-005 state machine ─── ADR-006 approvals ─── ADR-012 execution security ─── ADR-009 worktrees
ADR-007 adapters ─── ADR-018 routing ─── ADR-008 tokens ─── ADR-010 memory ─── ADR-013 resources
ADR-016 observability (cross-cutting)
```

## Consistency check (ADR ↔ PRD ↔ MVP)

| ADR | PRD requirements (Must unless noted) | MVP scope |
|---|---|---|
| 001 | NFR-001, NFR-010 | In |
| 002 | NFR-015, SEC-020 | In |
| 003 | FR-133, FR-138, UX-003, SEC-001/002 | In |
| 004 | NFR-004, NFR-017, FR-132, SEC-009 | In |
| 005 | FR-021 – FR-045 | In (custom profiles Should) |
| 006 | FR-025, FR-032, SEC-008, FR-156 | In (release approval when releases exist, post-MVP) |
| 007 | FR-050 – FR-066 | In |
| 008 | FR-070 – FR-086 | In (historical estimation Should → 0.4) |
| 009 | FR-090 – FR-102 | In |
| 010 | FR-120 – FR-128 (mostly Should/Could/Won't) | FTS5 for resources in; knowledge layer 0.4; local LLM/embeddings out |
| 011 | FR-036, FR-037, NFR-007 | In |
| 012 | SEC-004 – SEC-007, SEC-018 (Could) | Tiers 0–1 in; Tier 2 post-MVP |
| 013 | FR-110 – FR-118 | In (PDF Should, images Could) |
| 014 | LIC-001 – LIC-006 | Entitlements abstraction in; license files 1.0 |
| 015 | PRD §22 | Seams only |
| 016 | FR-130 – FR-140 | In |
| 017 | NFR-004, NFR-005, FR-099 | In |
| 018 | FR-057 – FR-060, FR-064 (Could) | Rules in; scoring 0.4 |
