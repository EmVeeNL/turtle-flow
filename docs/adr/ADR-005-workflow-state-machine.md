# ADR-005: Workflow State Machine

- **Status:** Accepted
- **Date:** 2026-10-24

## Context

Jobs move through a lifecycle with approvals, gates, loops and backward transitions ([WORKFLOW.md](../WORKFLOW.md)). Profiles (`fast`, `standard`, `strict`) vary the stages. The core principle is that Rust — not agents — controls transitions (PRD §13).

## Problem Statement

How should the workflow be modeled and executed so that it is deterministic, reviewable, testable, recoverable and configurable without becoming a general-purpose workflow engine?

## Decision

1. A **hand-written, explicit state machine** in `turtle-workflow`: `enum JobState`, `enum JobCommand`, and a pure function `decide(state, command, context) -> Result<Decision, TransitionRejected>`.
2. **Fixed stage graph** defined in code. Profiles are data that **select** optional stages, gates, approvals and limits; they **cannot add states or edges**.
3. **Guards** are named pure predicates evaluated over a precomputed `DecisionContext`; rejections carry the guard name.
4. **Holds** (pause, budget wait, rate-limit wait) are overlays, not states; `blocked` is a state with recorded `allowed_targets`.
5. **Release** is a separate aggregate with its own state machine.
6. The full transition table is snapshot-tested; property tests check invariants under random command sequences.

## Considered Alternatives

| Alternative | Assessment |
|---|---|
| External workflow engine (Temporal, Restate, Cadence) | Durable execution is attractive, but requires a server, adds a heavy dependency, and moves control logic into a different paradigm; overkill locally. Revisit for Phase 2 only if the server needs it |
| Generic DSL/BPMN-like user-defined workflows | Maximum flexibility, but invites invalid/unsafe workflows and bypassing approvals; hard to test exhaustively |
| State machine crates (`statig`, `rust-fsm`) | Helpful macros, but our guards need rich context and effects; a plain `match` is clearer and fully under control |
| Agent-driven orchestration (LLM decides next step) | Violates the core principle; non-deterministic |

## Rationale

A fixed graph with data-driven profiles provides necessary flexibility while keeping safety properties (e.g. "no delivery without acceptance") provable by tests. Pure `decide` makes the engine trivially replayable and testable.

## Consequences

- Adding a new stage requires a code change and a new release (intentional).
- Custom profiles are limited; some user requests will be "not supported by design".
- The `DecisionContext` must be computed before `decide` (IO happens outside), which slightly complicates the writer task.

## Risks

- Transition table grows large and hard to review.
- Profile combinations produce untested paths.

## Mitigations

- Table snapshot in PRs; grouped guard helpers; documentation generated from the table.
- Property tests over random profiles; profile validation rules ([WORKFLOW.md §5](../WORKFLOW.md#5-workflow-profiles)).

## Related ADRs

[ADR-006](ADR-006-supervised-autonomy-approvals.md), [ADR-011](ADR-011-job-scheduling-concurrency.md), [ADR-017](ADR-017-recovery-idempotency-crash-safety.md)
