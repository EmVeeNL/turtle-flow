# ADR-008: Token Intelligence and Budget Management

- **Status:** Accepted
- **Date:** 2026-10-24

## Context

Token management is a product differentiator (brief §7). Users need estimates before work, enforced budgets during work, and honest accounting after. Provider data is incomplete: usage may be per-turn, final-only or absent; quota is usually unknown (A-03).

## Problem Statement

How do we account, estimate, reserve and enforce token usage reliably given incomplete and heterogeneous provider data?

## Decision

1. **Ledger-based accounting:** append-only `ledger_entries` (`reserve`, `settle`, `release`, `adjust`, `limit_change`) per budget; budget balances are projections ([TOKEN-INTELLIGENCE.md §4](../TOKEN-INTELLIGENCE.md#4-budgets-reservations-and-the-ledger)).
2. **Reservations before every run** at P90 × (1 + confidence-dependent buffer), atomically across task/job/project/global budgets; settlement to actuals at run end.
3. **Provenance on every number:** `reported`, `estimated`, `unavailable`; quota status `known | partial | unknown`. Types enforce this in API responses.
4. **Estimation:** deterministic complexity scoring + hierarchical historical buckets over log-tokens with shrinkage, Monte Carlo aggregation for job totals, calibration feedback toward 80% P10–P90 coverage. MVP ships priors and heuristics; history-based estimation is enabled in 0.4 (data collected from MVP).
5. **Enforcement:** hard budgets block new runs (hold `awaiting_budget`); mid-run termination only where usage streams. Overruns are recorded truthfully.
6. **Cost:** versioned price tables; billing mode per provider (`api_metered`, `subscription` → "API-equivalent", `enterprise_contract`).

## Considered Alternatives

| Alternative | Assessment |
|---|---|
| Counters on budget rows only | Simple but not auditable; race-prone; no reservations |
| Post-hoc tracking only (no reservations) | Cannot prevent overspend with concurrent runs |
| ML models (gradient boosting) for estimation in MVP | Insufficient data; opaque; the hierarchical statistical model is explainable and data-efficient |
| LLM self-estimates of token needs | Unreliable, costs tokens; used at most as a bounded feature |
| Treat subscription usage as free | Hides real resource consumption and rate-limit pressure; API-equivalent cost is a useful common unit |

## Rationale

Reservations make concurrent enforcement correct. Provenance prevents the most damaging failure mode — false precision. Simple statistics with calibration are robust with little data.

## Consequences

- Budgets can be exceeded by one run when the provider does not stream usage (documented, visible).
- Users must maintain price tables for accurate costs (defaults flagged "verify").

## Risks

- Early estimates are wide and may annoy users or trigger too many approvals.
- Provider usage semantics differ (e.g. cached tokens counted differently).

## Mitigations

- Confidence labels and buffers; thresholds tuned per profile; priors from the evaluation suite.
- Per-adapter normalization documented and tested with fixtures; discrepancy warnings when reported cost and computed cost diverge.

## Related ADRs

[ADR-007](ADR-007-provider-adapter-architecture.md), [ADR-018](ADR-018-model-routing-and-evaluation.md), [ADR-006](ADR-006-supervised-autonomy-approvals.md), [ADR-011](ADR-011-job-scheduling-concurrency.md)
