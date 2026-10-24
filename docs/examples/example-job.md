---
# Job file for: turtle job create -p payments-api --file example-job.md
schema: turtle.job.v1
title: Add per-client rate limiting to the public API
type: feature
profile: standard
priority: high
budget:
  limit_tokens: 1200000
  hard: true
resources:
  - path: docs/api/openapi.yaml
    trust: internal
    pin: false
  - path: docs/adr/0007-rate-limiting.md
    trust: trusted
    pin: true
blocked_by: []
overrides:
  implementer: codex/<model-id>      # optional; omit to let the router decide
labels:
  ticket: PAY-1182
---

## Goal

Protect the public `/v1/*` endpoints from abusive clients by enforcing per-API-key rate limits, as decided in ADR-0007.

## Requirements

- Token-bucket limiting keyed by API key; default 100 requests/minute, burst 20.
- Limits configurable per key tier via the existing `ClientTier` table.
- Exceeding the limit returns HTTP 429 with `Retry-After` and the standard error body.
- Health and metrics endpoints are exempt.
- Expose a Prometheus counter `api_rate_limited_total{tier}`.

## Out of scope

- Distributed (multi-instance) rate limiting — a follow-up job will add Redis-backed state.
- Changes to authentication.

## Acceptance hints

- Integration tests for 429 behavior and `Retry-After`.
- Existing test suite remains green; no new dependencies except a well-known rate limiting crate if justified.

## Notes for Turtle Flow

The middleware stack lives in `src/http/middleware/`. Follow `docs/CONVENTIONS.md` for error bodies.
