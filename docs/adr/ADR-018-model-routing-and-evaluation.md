# ADR-018: Model Routing and Evaluation

- **Status:** Accepted
- **Date:** 2026-10-24

## Context

Turtle Flow selects provider/model per agent run based on complexity, category, risk, capabilities, history, estimated tokens, preferences, budget and availability (brief §6). The brief forbids hardcoding beliefs like "Claude is better at architecture, Codex at coding".

## Problem Statement

How do we route runs to providers/models in a way that is explainable, respects policy and budgets, and improves with evidence rather than assumptions?

## Decision

1. **Two-stage router:** hard filters (policy, capabilities, health/auth, budget headroom, overrides) → scoring of remaining candidates. Every decision is persisted with candidates, scores and reason ([AGENT-PROTOCOL.md §6](../AGENT-PROTOCOL.md#6-provider-selection-and-routing-sequence)).
2. **No vendor assumptions in code.** Quality priors come from (a) user-defined **capability tiers** mapping roles × complexity to tiers, and tiers to models in configuration, and (b) measured metrics.
3. **MVP:** rule-based scoring with tiers, user preferences and cost; history only displayed.
   **0.4:** historical metrics per bucket `(role, complexity, language, provider, model)` with shrinkage enter the score when `n ≥ 20`:
   `quality = P(first-pass gates) × (1 − blocker_rate)`; `value = quality / E[tokens incl. retries]`.
   **Later:** contextual bandit (Thompson sampling over Beta posteriors of success) with ε-bounded exploration only on low-risk runs (XS/S, non-strict), never for reviewer of strict jobs.
4. **Evaluation framework:** offline suite (`turtle eval`) of fixed tasks in fixture repos with hidden acceptance tests, run per provider/model/CLI version; online metrics from real jobs; regression alerts.
5. **Independence rule:** strict profiles require reviewer ≠ implementer provider (or model family where only one provider is available — configurable).

## Considered Alternatives

| Alternative | Assessment |
|---|---|
| Static mapping in code | Encodes unverified assumptions; stale quickly |
| LLM-based router ("ask a model which model to use") | Costs tokens, non-deterministic, unexplainable |
| Always cheapest / always strongest | Wastes money or quality |
| Full bandit from day one | No data; exploration costs real money on real work |

## Rationale

Filters guarantee safety; transparent scoring is explainable; evidence accumulates naturally from normal use and the eval suite.

## Consequences

- Users must configure tiers initially (sensible defaults shipped as editable config, labeled as defaults).
- Online metrics are confounded (harder jobs routed to stronger models); offline eval provides unconfounded comparisons.

## Risks

- Metric gaming (e.g. a model that "passes" weak tests).
- Small samples lead to noisy rankings.

## Mitigations

- Include review blocker rate and user acceptance in quality; hidden tests in eval suite.
- Shrinkage, minimum sample sizes, confidence display, and manual overrides.

## Related ADRs

[ADR-007](ADR-007-provider-adapter-architecture.md), [ADR-008](ADR-008-token-intelligence-budgets.md), [ADR-010](ADR-010-project-memory-context-retrieval.md)
