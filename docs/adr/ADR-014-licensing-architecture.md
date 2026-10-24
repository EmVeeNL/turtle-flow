# ADR-014: Licensing Architecture

- **Status:** Accepted
- **Date:** 2026-10-24

## Context

Four tiers (Free, Team, Business, Enterprise) are planned ([LICENSING.md](../LICENSING.md)). The product is local-first and must work offline; Enterprise needs air-gapped operation. The MVP ships only the Free tier.

## Problem Statement

How do we implement license verification and feature gating that is secure enough for compliance-driven buyers, friendly to users, and does not burden the MVP?

## Decision

1. **Entitlements service** (`Entitlements::check(Feature)`) is the single gating point, present from the MVP with a `FreeLocalEntitlements` implementation (LIC-002).
2. **Signed license files:** JSON payload + **Ed25519** signature over canonical JSON, verified offline with public keys embedded in the binary (key IDs for rotation; revocation list shipped with releases, optional online refresh).
3. **No license needed for Free;** no account, no telemetry (LIC-001, LIC-005).
4. **Graceful enforcement:** expiry → 30-day grace → paid features read-only; never lock data, history or exports; never block logins for seat overages (LIC-004).
5. **Optional binding** to a server installation ID (Enterprise), never to hardware fingerprints.
6. License stored in the OS keychain (local) or server secret store.

## Considered Alternatives

| Alternative | Assessment |
|---|---|
| Online activation server mandatory | Breaks offline/air-gapped; hostile UX; adds infrastructure dependency |
| Hardware-bound keys | Breaks on VM migrations and hardware changes |
| Obfuscated/DRM checks | Hurts reliability and trust; ineffective against determined crackers |
| Separate binaries per tier | Distribution complexity; harder upgrades |

## Rationale

Ed25519 offline verification is simple, fast and robust. Target customers pay for compliance and support, so friendly enforcement is commercially sufficient.

## Consequences

- Cracked builds possible; accepted.
- Key management and rotation process required at the vendor.

## Risks

- Signing key compromise.
- Clock manipulation to extend expiry.

## Mitigations

- Offline HSM-backed signing key; rotation and revocation lists.
- Monotonic "last seen" timestamp stored locally; clock skew > 7 days triggers a warning (not a lockout).

## Related ADRs

[ADR-015](ADR-015-future-server-deployment.md), [ADR-001](ADR-001-local-first-modular-monolith.md)
