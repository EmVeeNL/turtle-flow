# ADR-015: Future Server Deployment

- **Status:** Accepted (direction); detailed design deferred to Phase 2
- **Date:** 2026-10-24

## Context

Teams and organizations need a shared Turtle Flow deployable on internal servers, external hosts, private clouds, and potentially a managed cloud ([ROADMAP.md §5](../ROADMAP.md#5-phase-2-turtle-flow-server-architecture)). The MVP must not preclude this.

## Problem Statement

What is the target architecture for server deployment, and which MVP constraints keep the path open?

## Decision

1. A separate **`turtle-server`** distribution reusing core crates (workflow, policy, tokens, scheduler, agents, git, exec, context, artifacts).
2. **PostgreSQL** for state/events/ledger (repository trait implementations), **S3-compatible object storage** for blobs.
3. **Stateless API replicas**; scheduler **leader-elected** via Postgres advisory lock.
4. **Pull-based worker nodes** connecting outbound over **mTLS** to claim leases (`SKIP LOCKED`), stream events, upload artifacts; workers always run Tier 2+ isolation.
5. **AuthN:** OIDC (Business), SAML/SCIM (Enterprise); **AuthZ:** RBAC + project grants + approval policies in `turtle-policy`.
6. Server deployment available for **Team (single node, ≤ 3 workers), Business, Enterprise** ([LICENSING.md §6](../LICENSING.md#6-server-deployment-by-tier)).
7. MVP provisions: repository traits, command API, actor attribution on events, `Executor` abstraction, serialized `AgentRequest`, `Entitlements`, `authorize()` hook.

## Considered Alternatives

| Alternative | Assessment |
|---|---|
| Server = many local daemons synchronized | Conflict-prone; no central policy |
| Push-based workers (server connects to workers) | Requires inbound connectivity to workers; harder behind firewalls |
| Kubernetes-only | Excludes small teams; Compose/systemd needed too |
| SQLite on server | Fine for tiny teams; no HA; Postgres chosen for multi-replica (SQLite may remain an option for Team single-node) |

## Rationale

Reusing the same core guarantees identical workflow semantics locally and on servers; pull-based workers fit enterprise networks.

## Consequences

- Store abstraction must be maintained for two databases.
- Remote context packaging must avoid shipping more data to workers than needed.

## Risks

- Abstractions designed now are wrong for later needs.
- Provider ToS may restrict shared/centralized use of subscriptions (OQ-01).

## Mitigations

- Keep provisions minimal (seams, not frameworks); revisit with design partners before Phase 2.
- Support API-key/enterprise provider accounts for server mode.

## Related ADRs

[ADR-001](ADR-001-local-first-modular-monolith.md), [ADR-004](ADR-004-sqlite-persistence-event-logging.md), [ADR-011](ADR-011-job-scheduling-concurrency.md), [ADR-014](ADR-014-licensing-architecture.md), [ADR-012](ADR-012-agent-execution-security.md)
