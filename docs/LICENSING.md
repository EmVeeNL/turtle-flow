# Licensing and Commercial Strategy

| Field | Value |
|---|---|
| Document | LICENSING.md |
| Status | Proposal. **All prices are hypotheses**, not validated market prices. Architecture decision in [ADR-014](adr/ADR-014-licensing-architecture.md); server deployment in [ADR-015](adr/ADR-015-future-server-deployment.md). |
| Related | [PRD.md §21](PRD.md#21-licensing-requirements) (LIC-xxx), [ROADMAP.md](ROADMAP.md) |

---

## 1. Commercial Principles

1. **Never tax AI usage.** Users already pay AI providers. Turtle Flow license fees are independent of tokens consumed; we never resell or mark up provider usage. The UI and invoices keep the two strictly separate (LIC-006).
2. **The individual developer experience is free and complete.** A single developer on their own machine gets the full supervised workflow, both providers, all built-in profiles, token intelligence and local analytics. Adoption drives everything else.
3. **Charge for coordination, governance and scale.** Revenue comes from what teams and organizations need and individuals don't: shared state, roles, approval policies, central budgets, audit, SSO, server deployment, worker scaling, support.
4. **Safety features are not paywalled.** Sandboxing, secret redaction, approval integrity and audit hash chains are in every tier. Charging for safety would push users to unsafe setups and hurt the brand.
5. **Enforcement is respectful.** Offline-verifiable license files, generous grace periods, no data lock-in, no phone-home requirement (LIC-001, LIC-004, LIC-005).

## 2. Value Metric

| Candidate | Assessment |
|---|---|
| Per token / per job | Rejected: conflates with provider costs, punishes success, unpredictable bills |
| Per project | Rejected as primary: agencies have many small projects; encourages monorepo hacks |
| Per concurrent job | Rejected as primary: local hardware and provider limits already constrain it; feels artificial |
| **Per active seat (user)** | **Chosen** for Team/Business: aligns with collaboration value, familiar to buyers |
| Platform fee + seats | Enterprise: covers deployment/support overhead |
| Per worker node | Secondary limit for Team (caps infrastructure-heavy use at low price) |

An **active seat** = a user who created a job, approved, or reviewed in the last 30 days. Viewers (read-only dashboards) are free in Business/Enterprise to encourage visibility.

## 3. Tier Overview

| | **Free** | **Team** | **Business** | **Enterprise** |
|---|---|---|---|---|
| Audience | Individual developers | Small teams (2–25) | Engineering orgs (25–500) | Large/regulated orgs |
| Deployment | Local (single machine) | Local + self-hosted Turtle Flow Server (single node) | Self-hosted server, multiple workers, private cloud | Any incl. air-gapped, HA, (future) managed cloud |
| Users | 1 | 2–25 seats | 10+ seats | Custom |
| Price hypothesis | $0 | $19/seat/mo annual ($24 monthly) | $49/seat/mo annual | Custom; indicative from $25k/yr platform fee + seats |
| License | None required | Signed license file | Signed license file | Signed license file (offline), contract |
| Support | Community | Email, best effort (2 business days) | Priority (1 business day), onboarding call | SLA (e.g. 4 h P1), named contact |

## 4. Detailed Feature Matrix

Legend: ✓ included · — not included · *L* limited (value shown) · ◐ planned later phase

| Area | Feature | Free | Team | Business | Enterprise |
|---|---|---|---|---|---|
| **Core workflow** | Supervised workflow (spec → approval → plan → implement → gates → review → verify → accept → deliver) | ✓ | ✓ | ✓ | ✓ |
| | Built-in profiles fast/standard/strict | ✓ | ✓ | ✓ | ✓ |
| | Custom profiles (derived) | ✓ | ✓ | ✓ | ✓ |
| | Organization-enforced profiles (cannot be relaxed by projects) | — | — | ✓ | ✓ |
| | Release workflow (notes, tags) | ✓ | ✓ | ✓ | ✓ |
| | Deploy hooks & rollback automation | — | ✓ | ✓ | ✓ |
| **Projects & jobs** | Number of projects | Unlimited | Unlimited | Unlimited | Unlimited |
| | Concurrent agent runs | *L* 3 (configurable up to hardware) | *L* 3 per worker | Unlimited (per worker capacity) | Unlimited |
| | Worker nodes (server) | — | *L* up to 3 | Unlimited | Unlimited |
| | Job dependencies, priorities | ✓ | ✓ | ✓ | ✓ |
| **Providers** | Claude Code, Codex adapters | ✓ | ✓ | ✓ | ✓ |
| | Additional community/first-party adapters | ✓ | ✓ | ✓ | ✓ |
| | Custom adapters (private, supported) | — | — | — | ✓ |
| | Provider credential isolation per project | — | ✓ | ✓ | ✓ |
| **Token intelligence** | Usage tracking, budgets, reservations, preflight | ✓ | ✓ | ✓ | ✓ |
| | Historical estimation & routing | ✓ | ✓ | ✓ | ✓ |
| | Analytics retention | 90 days detail, aggregates forever | 1 year | 3 years | Custom |
| | Team/org budgets, cost centers, chargeback reports | — | *L* team budget | ✓ | ✓ |
| | Routing evaluation suite & cross-project benchmarking | ✓ (local) | ✓ | ✓ + org-wide | ✓ |
| **Collaboration** | Multi-user, shared projects | — | ✓ | ✓ | ✓ |
| | Roles | — | *L* admin/member/viewer | Full RBAC + project grants | Full RBAC + custom roles |
| | Approval policies (four-eyes, required approvers per path) | — | *L* basic (approver ≠ creator) | ✓ | ✓ |
| | Shared approval queue, mobile-friendly approvals | — | ✓ | ✓ | ✓ |
| | Notifications (Slack/Teams/email/webhooks) | Desktop only | ✓ | ✓ | ✓ |
| **Access** | Local dashboard | ✓ | ✓ | ✓ | ✓ |
| | Remote access (server, TLS) | — | ✓ | ✓ | ✓ |
| | Local HTTP API | ✓ | ✓ | ✓ | ✓ |
| | Server API tokens, webhooks, CI integration | — | *L* | ✓ | ✓ |
| **Security & compliance** | Sandboxing tiers 0–2, redaction, approval integrity, hash-chained audit | ✓ | ✓ | ✓ | ✓ |
| | Audit log export (JSON/CSV), SIEM streaming | — | — | ✓ | ✓ |
| | SSO OIDC | — | — | ✓ | ✓ |
| | SAML, SCIM provisioning | — | — | — | ✓ |
| | Organization policies (allowed providers/models, data residency hints, mandatory strict paths) | — | — | ✓ | ✓ |
| | Air-gapped offline licensing, custom key rotation | — | — | — | ✓ |
| | HA deployment reference architecture | — | — | — | ✓ |
| **Support** | Community forum | ✓ | ✓ | ✓ | ✓ |
| | Email support | — | ✓ | ✓ priority | ✓ SLA |
| | Security questionnaire, DPA, indemnification | — | — | DPA | ✓ |

### 4.1 What stays free and why

- **Full single-user workflow and token intelligence:** the core value proposition; restricting it would block adoption and word of mouth.
- **Unlimited projects:** artificial project limits annoy individuals and agencies without capturing real value.
- **All safety features:** see principle 4.
- **Local API:** enables scripting and community integrations; network effects (OQ-09 tracks a counter-argument).
- **Concurrency cap of 3 (configurable):** not a monetization lever; protects users from provider rate limits by default. Users can raise it.

### 4.2 What justifies paid tiers

- **Team:** shared Turtle Flow (server), shared approvals, roles, remote access, notifications, team budgets — value appears only with ≥ 2 people.
- **Business:** governance (SSO, RBAC, approval policies, org policies, audit export), scale (unlimited workers), analytics for managers (chargeback, cross-project).
- **Enterprise:** compliance and deployment constraints (SAML/SCIM, air-gap, HA, SLA, legal terms).

## 5. Pricing Hypotheses and Validation Plan

| Hypothesis | Rationale | Validation method |
|---|---|---|
| H1: Teams pay ~$19–29/seat/mo for coordination features | Comparable to developer collaboration tools; small relative to AI provider spend per developer | Pricing page A/B, 15 design-partner interviews (Van Westendorp) |
| H2: Business buyers pay ~$40–60/seat/mo for SSO + governance | Typical SSO/governance uplift in dev tools | Design partners, procurement feedback |
| H3: Enterprise requires $25k+/yr platform fee | Covers support, security reviews, deployment help | First 3 enterprise pilots |
| H4: A meaningful share of Free users convert when their team grows | Adoption-led motion | Opt-in telemetry + license sales attribution |
| H5: Air-gapped/offline licensing is a hard requirement for regulated buyers | Common in defense/finance/healthcare | Enterprise interviews |

Discounts (hypotheses): education/open-source maintainers free Team; startups 50% first year; annual prepay ~20% discount.

## 6. Server Deployment by Tier

| Deployment | Free | Team | Business | Enterprise |
|---|---|---|---|---|
| Local single-user | ✓ | ✓ | ✓ | ✓ |
| Self-hosted server on internal machine | — | ✓ (single node, ≤ 3 workers) | ✓ | ✓ |
| Externally hosted VM (user's cloud account) | — | ✓ | ✓ | ✓ |
| Private cloud / Kubernetes (Helm chart) | — | — | ✓ | ✓ |
| HA (multiple server replicas, Postgres HA) | — | — | — | ✓ |
| Air-gapped | — | — | — | ✓ |
| Managed cloud (vendor-hosted, future) | — | ◐ | ◐ | ◐ |

Rationale: server deployment is where collaboration value arises, so it starts at Team; operational complexity (Kubernetes, HA) correlates with Business/Enterprise needs and support costs.

## 7. License Architecture (summary of ADR-014)

### 7.1 License file

```json
{
  "payload": {
    "schema": "turtle.license.v1",
    "license_id": "lic_01JB8…",
    "licensee": { "org": "Example GmbH", "contact": "it@example.com" },
    "tier": "business",
    "seats": 60,
    "worker_limit": null,
    "features": ["server", "sso_oidc", "rbac", "audit_export", "org_policies"],
    "deployment": ["local", "server"],
    "issued_at": "2026-11-01T00:00:00Z",
    "not_before": "2026-11-01T00:00:00Z",
    "expires_at": "2027-11-01T00:00:00Z",
    "grace_days": 30,
    "server_binding": null
  },
  "key_id": "fk-2026-1",
  "signature": "base64(ed25519(canonical_json(payload)))"
}
```

- Verified offline with Ed25519 public keys embedded in the binary (multiple `key_id`s for rotation; revocation list shipped with releases and optionally fetched).
- `server_binding` (optional, Enterprise) binds to an installation ID generated at server setup — never to hardware fingerprints, which break on VM migration.
- Stored in the OS keychain (local) or server secret store; the payload (not secret) is visible in the Licensing screen.

### 7.2 Entitlements service

```rust
pub enum Feature { Server, MultiUser, Rbac, ApprovalPolicies, SsoOidc, SsoSaml, Scim, AuditExport,
                   OrgPolicies, Workers(u32), RetentionDays(u32), DeployHooks, CredentialIsolation }

pub trait Entitlements: Send + Sync {
    fn check(&self, feature: Feature) -> Entitlement;   // Granted | Grace{until} | Denied{reason}
    fn tier(&self) -> Tier;
}
```

All gating goes through this service (LIC-002). In the MVP, a `FreeLocalEntitlements` implementation grants every single-user feature; multi-user features do not exist yet.

### 7.3 Enforcement behavior

| Situation | Behavior |
|---|---|
| No license | Free tier; nothing nags |
| Valid license | Paid features enabled |
| Expired, within grace (30 days) | Banner to admins; all features work |
| Expired, after grace | Paid features become read-only (e.g. cannot add users, SSO continues to work for existing users for an additional 14 days to avoid lockouts); **all data, history and exports remain accessible**; jobs in progress finish |
| Seats exceeded | Warning + true-up at renewal; never blocks logins |
| Signature invalid | Treated as no license; security event logged |

No hostile measures: no remote kill switch, no obfuscated checks that break builds, no telemetry requirement. Determined pirates are not the target customer; companies that need Business/Enterprise features buy licenses for compliance reasons.

## 8. Open-Source / Source-Availability Strategy (OQ-02)

**Decision (2026-10-25, product owner): the source stays private (option C, proprietary).** The repository remains private because of the paid tiers. Consequences to plan for: the trust cost named in the table below is real for a tool that runs agents on users' source code, so compensate with what can be public without the code: the security model (`SECURITY.md`), the verifiable audit log (`turtle audit verify`), documented network behavior (none beyond the providers the user already uses), and, later, an independent security review or source escrow for Business and Enterprise customers. **There will be a free tier (confirmed 2026-10-25): a free binary of the proprietary product, no license key needed.** The terms of use (an end-user license agreement: personal and commercial use allowed within the Free feature limits, no redistribution, no reverse engineering, provider terms remain the user's responsibility) still have to be written and belong with the launch plan, together with the legal review of OQ-01. The options below are kept for the record.

| Option | Pros | Cons |
|---|---|---|
| A. Open-core: Apache-2.0 local core; proprietary server/enterprise crates | Trust, community adapters, security review by users | Competitors can fork the core |
| B. Source-available (e.g. BSL/FSL) for everything, converting to OSS after N years | Protects business; still auditable | Some community resistance |
| C. Proprietary, free binary | Simple | Lower trust for a tool that runs agents on source code |

**Recommendation (proposal):** Option A or B. Security-sensitive users strongly prefer auditable code for a tool that executes AI agents against their repositories. The crate layout already separates `turtle-server`/enterprise features (Phase 2) from the local core, which keeps both options open.

## 9. MVP vs. Later

| Item | MVP | Post-MVP (1.0) | Phase 2 |
|---|---|---|---|
| Free tier, no license needed | ✓ | ✓ | ✓ |
| `Entitlements` abstraction | ✓ (all granted) | ✓ | ✓ |
| License file verification | — | ✓ | ✓ |
| Licensing screen | — (placeholder) | ✓ | ✓ |
| Seat counting | — | — | ✓ |
| Paid features (server, SSO, RBAC…) | — | Team features preview | ✓ |

## 10. Risks

| Risk | Mitigation |
|---|---|
| Free tier too generous → low conversion | Paid value concentrated in collaboration/governance where willingness to pay exists; monitor H4 |
| Provider ToS changes make orchestration of subscriptions impossible | Support API-key usage paths; value is in orchestration, not access |
| Providers ship similar orchestration natively | Differentiate on vendor neutrality, approvals/audit, token intelligence, self-hosting |
| License cracking | Accept; focus on compliance-driven buyers |
| Confusion between Turtle Flow fees and AI costs | Separate UI sections, explicit labels, documentation (LIC-006) |
