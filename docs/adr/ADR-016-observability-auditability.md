# ADR-016: Observability and Auditability

- **Status:** Accepted
- **Date:** 2026-10-24

## Context

Users must see what Turtle Flow and agents did (FR-132 – FR-140), and organizations need tamper-evident audit trails (SEC-009). Debugging long-running, multi-process jobs requires correlation across components.

## Problem Statement

What logging, tracing, metrics and audit mechanisms provide operational insight and trustworthy history without external infrastructure?

## Decision

1. **Three distinct channels:**
   - **Event log** (DB, hash-chained): the business/audit history — every state change, approval, run, gate, Git mutation, security event. Source for timelines and SSE.
   - **Structured logs** (`tracing`, JSON lines, rotated): technical diagnostics with spans `job`/`task`/`run`/`effect` carrying IDs.
   - **Transcripts** (blobs): full normalized + raw agent output per run, redacted, retention-managed.
2. **Metrics are derived** from the DB (analytics queries) in the MVP; an optional Prometheus/OTLP exporter is Phase 2.
3. **Lineage queries**: `turtle lineage <commit>` and job reports reconstruct the chain from requirement to delivery.
4. **`turtle audit verify`** validates the event hash chain and approval HMACs.
5. **Diagnostic bundle** (`turtle doctor --bundle`): redacted logs, config, versions, recent events for bug reports.

## Considered Alternatives

| Alternative | Assessment |
|---|---|
| Logs as the audit trail | Not structured or tamper-evident; rotation loses history |
| OpenTelemetry collector required | Extra infrastructure locally |
| Store transcripts in DB | Bloat; blobs better |

## Rationale

Separating audit (must be complete and durable) from diagnostics (best effort, rotated) avoids compromising either.

## Consequences

- Event types must be documented and versioned.
- Some duplication between events and logs (acceptable).

## Risks

- Transcripts leak sensitive content.
- Event volume from streaming output.

## Mitigations

- Redaction at capture; retention; export warnings.
- Agent output deltas are *not* events; they go to transcripts and a transient SSE channel; only milestones are events.

## Related ADRs

[ADR-004](ADR-004-sqlite-persistence-event-logging.md), [ADR-006](ADR-006-supervised-autonomy-approvals.md), [ADR-012](ADR-012-agent-execution-security.md), [ADR-017](ADR-017-recovery-idempotency-crash-safety.md)
