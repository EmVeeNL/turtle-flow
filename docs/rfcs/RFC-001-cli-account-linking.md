# RFC-001: CLI Account Linking and Expiring License Leases

| Field | Value |
|---|---|
| Status | Approved for implementation |
| Date | 2026-10-27 |
| Owners | Turtle Flow CLI and customer dashboard |
| Related | [ADR-001](../adr/ADR-001-local-first-modular-monolith.md), [ADR-014](../adr/ADR-014-licensing-architecture.md), [ROADMAP.md](../ROADMAP.md) |
| Protocol references | [OAuth 2.0 Device Authorization Grant (RFC 8628)](https://www.rfc-editor.org/rfc/rfc8628), [OAuth 2.0 Security Best Current Practice (RFC 9700)](https://www.rfc-editor.org/rfc/rfc9700) |

## 1. Summary

Add an explicit `turtle login` flow that links a local CLI installation to the user’s customer-dashboard account. The CLI uses a device authorization flow: it displays a short code, the user approves that device in the authenticated dashboard, and the CLI receives a device-bound, Ed25519-signed license lease.

An account may have at most **two active linked CLI installations**. The approved lease lifetime is **30 days**. At expiry, the CLI prompts the user to run `turtle login` and approve a new lease. The CLI does not hold a long-lived bearer or refresh token. The signed lease is cached locally and can be checked offline until expiry.

Linking transfers account and installation metadata only. Project data, source content, jobs, runs, transcripts, prompts, diffs, and usage history remain in local SQLite and are not sent to the customer dashboard.

## 2. Goals

- Let the CLI detect a missing or expired account license and tell the user how to link it.
- Make account linking usable from a terminal without entering a password or passkey into the CLI.
- Reuse the customer dashboard’s existing account authentication and protection requirements.
- Enforce two active linked CLI installations per account, including concurrent approvals.
- Give each linked installation a signed, expiring license lease that the CLI can verify offline.
- Support device listing and revocation from the customer dashboard.
- Keep local-only Free use working without an account, server dependency, or valid account lease.

## 3. Non-goals

- Synchronizing local projects, jobs, runs, or files to Cloudflare.
- Remote project management or remote execution.
- Binding licenses to hardware fingerprints.
- Making a CLI installation an authentication mechanism for the local daemon.
- Replacing the existing localhost dashboard login-code flow.

## 4. Terminology

- **Account license:** the single license record owned by a dashboard account. It describes the account’s entitlement.
- **Linked installation:** one CLI installation authorized by that account. An installation has a random identifier and a public key; it is not a hardware fingerprint.
- **Device authorization code:** short-lived secret the CLI uses to poll while a human approves a login. It is not a license and must not be placed in a URL.
- **User code:** short, human-readable code the user enters on the dashboard to identify the pending CLI request. It is not sufficient to retrieve a license by itself.
- **License lease:** the signed, installation-bound entitlement cached by the CLI. It expires and requires a new interactive authorization.

## 5. User experience

### 5.1 CLI

1. The CLI checks its locally cached lease and verifies its Ed25519 signature, installation binding, and expiry. It does not contact the dashboard on every normal command.
2. If the lease is absent or expired, the CLI reports that no current account license is linked and suggests `turtle login`. It continues to offer the existing local Free entitlement; expiry must not block access to local projects, history, or exports.
3. `turtle login` creates or reuses a local installation key, requests device authorization, prints `https://dash.turtle-flow.com/device` and the user code, and opens the URL when possible.
4. The CLI polls the token endpoint at the server-provided interval, showing a pending state. It handles `slow_down`, denial, expiry, network failure, and cancellation without tight polling.
5. On approval, the CLI verifies and atomically saves the signed lease. It reports the account and lease expiry, but never prints the device secret or private key.
6. To renew an expired lease, the user runs `turtle login` again. The same locally stored public key identifies the existing installation; the CLI verifies that the new lease is bound to its private-key thumbprint, and renewal does not consume another slot.

Suggested missing-license message:

> No current Turtle Flow account license is linked. Run `turtle login` to connect this CLI. Your local Free workflow and data remain available.

### 5.2 Customer dashboard

- `/device` accepts the user code, requires sign-in, and displays the CLI installation name and platform before approval.
- The dashboard requires the account’s existing protection (passkey or authenticator) before issuing or renewing a lease.
- The user can approve or deny the request. Approval shows the user which device they are authorizing.
- A **Linked CLI installations** section shows the two slots, device name/platform, last authorization or renewal, and a revoke action. Do not add periodic background heartbeats just to refresh a “last seen” field.
- If both slots are occupied, the approval page offers revocation of an existing installation before approving the new one.

## 6. Authorization protocol

Use the [OAuth 2.0 Device Authorization Grant](https://www.rfc-editor.org/rfc/rfc8628) pattern, adapted for Turtle Flow’s account license API.

### 6.1 Start authorization

`POST /api/cli/device/authorizations`

The CLI sends its installation public key, a display name, OS/platform, and CLI version. It sends no project or user-workspace data. The endpoint returns:

```json
{
  "device_code": "high-entropy-polling-secret",
  "user_code": "K7PM-W3CX",
  "verification_uri": "https://dash.turtle-flow.com/device",
  "expires_in": 600,
  "interval": 5
}
```

The CLI keeps `device_code` in memory only while the authorization is pending. The user enters `user_code` on the dashboard; the URL contains no code or bearer token. Device authorization expires after 10 minutes and can be used once. Store only `SHA-256(device_code)` and `HMAC-SHA-256(CLI_DEVICE_CODE_PEPPER, user_code)`; keep `CLI_DEVICE_CODE_PEPPER` as a separate Worker secret. Apply rate limits and a strict attempt limit to user-code entry.

### 6.2 Approve or deny

`POST /api/cli/device/authorizations/approve` and `/deny` require an authenticated dashboard session, CSRF protection, and completed account protection. Approval is transactional:

1. Lock the account row to serialize concurrent slot checks.
2. If the installation ID is already active for this account, renew it without consuming another slot. Otherwise count active installations and require fewer than two.
3. Ensure the account has a license. If it does not, offer creation of the single free license in the approval flow; create it only after the user confirms.
4. Create or renew the installation and associate it with the account’s active license.
5. Mark the pending authorization approved and make it available to the matching CLI poller.

If two slots are active, return a conflict and let the user revoke one before retrying. A race between two browser approvals must never create a third active installation.

### 6.3 Poll and receive lease

`POST /api/cli/device/tokens`

The CLI polls with its high-entropy `device_code`. Responses follow device-flow semantics: `authorization_pending`, `slow_down`, `access_denied`, `expired_token`, or success. On success, the CLI verifies that the returned lease is bound to its locally stored installation-key thumbprint. Use `Cache-Control: no-store` and TLS for every endpoint.

On success, the endpoint returns a signed `turtle.license.v1` lease bound to the account license and installation identifier/public-key thumbprint. The CLI verifies it against the embedded public-key set before storing it. Device codes and user codes are single-use; store only hashes of these values server-side. Do not log codes, signatures, or token response bodies.

### 6.4 Revoke installation

`GET /api/cli/installations` lists the signed-in user’s installations. `DELETE /api/cli/installations/{installationId}` revokes one installation and its ability to obtain new leases. Revocation is immediate for online requests. A CLI that is offline can continue using its already-issued lease until that lease expires; the 30-day proposed maximum bounds this offline revocation delay.

These paths are an API contract proposal. They should be implemented as HTTP endpoints in the customer dashboard Worker; the `/device` page is the human approval UI. TanStack server functions remain appropriate for the dashboard’s own UI calls, but are not the CLI protocol.

## 7. Credentials and lifetime policy

| Credential | Proposed lifetime | Storage/use |
|---|---:|---|
| User code | 10 minutes | Displayed to user; one-time; attempt-limited; server stores keyed hash |
| Device code | 10 minutes | CLI memory only; one-time poll authorization; server stores hash |
| Installation private key | Until unlink/reinstall | OS credential store; used locally to bind/verify the lease for this installation; never uploaded |
| API access token | None persistent in the initial design | Device code is transient; no long-lived bearer token is issued to the CLI |
| Signed license lease | 30 days (proposed) | Local license store; Ed25519 verification; renew through `turtle login` |

The 30-day period is a proposed policy, not an accepted product decision. It makes the requested reauthentication interval explicit and limits the period an offline CLI can use a revoked lease. The account’s license record remains separate from each installation lease. Each successful login issues a lease whose expiry is absolute; renewal must require an explicit CLI login and dashboard approval rather than silently extending an unbounded refresh token.

The license payload should include at least `schema`, `license_id`, `installation_id`, `installation_key_thumbprint`, `tier`, `issued_at`, `not_before`, `expires_at`, `features`, and `key_id` in the envelope. CLI checks expiry before treating account entitlements as current. Clock rollback detection should follow ADR-014’s warning behavior and must not damage local data.

## 8. Storage model

Add two server-side concepts:

1. **Pending device authorization:** hashed device code, keyed hash of user code, installation public key, display name/platform/version, state, creation/expiry times, and approved account/license IDs. Delete expired or consumed rows promptly.
2. **Linked CLI installation:** random ID, user/license IDs, public key, display metadata, created/last-authorized times, and revoked timestamp. Keep only fields needed for authorization, slot limits, display, and audit.

The server stores no CLI private key, refresh token, project name, repository URL, job/run data, prompt, diff, transcript, or usage ledger. Audit events record linking, renewal, denial, and revocation without embedding credentials.

## 9. Security and failure behavior

- A short user code identifies a pending request but never grants credentials on its own. Require a logged-in, protected account to approve it.
- Keep polling secrets out of URLs, browser local storage, logs, analytics, and error reports. Hash them at rest and expire them promptly.
- Rate-limit authorization creation, polling, and user-code attempts by account and network origin. Return `slow_down` to clients that poll too quickly.
- Check the active-installation limit inside the approval transaction, not only in the UI.
- Bind a lease to a random installation identity and key; never use MAC addresses, disk IDs, or other hardware fingerprints.
- Keep device authorization endpoints separate from Better Auth session endpoints and the local daemon’s one-time dashboard login code.
- On dashboard/network outage, retain local-only Free behavior and local data. Do not make ordinary CLI commands phone home.
- On a bad signature or unsupported key ID, ignore the account lease, explain the problem, and offer login/support guidance. Never accept unsigned or unknown-key leases.
- On expiry or revocation, account-linked entitlements are unavailable until reauthorization. Local Free capabilities and all user data remain accessible.

## 10. Alternatives considered

### Put a token in the browser link

Rejected. Bearer credentials in URLs can leak through browser history, referrers, logs, screenshots, and copied links. The browser link should be a fixed verification page; the short user code is entered there.

### CLI username/password or passkey authentication

Rejected. The terminal should not collect account passwords or authenticator secrets. Passkey and MFA ceremonies belong in the browser’s existing protected dashboard session.

### Long-lived refresh token with silent renewal

Deferred. It avoids periodic user interaction but conflicts with the requested forced login interval and creates a high-value persistent bearer secret. The initial proposal uses a 30-day signed lease and explicit reauthorization instead.

### Store local project data in the customer dashboard

Rejected for this feature. Free local project data stays in local SQLite. A future same-computer display should read directly from the local daemon; it is outside this token/linking protocol.

## 11. Compatibility with current decisions and implementation

- ADR-014 says Free needs no account or license. This RFC preserves that local Free mode and treats account linking as optional. A missing/expired account lease displays the login instruction; it does not block local work.
- The current customer dashboard has one account-level license record and a signed download. A linked CLI lease must be installation-bound and expiring; it is not the same object as the account’s durable license row.
- The CLI currently has a separate localhost dashboard code flow in `turtle-api`. Keep it unchanged; the new account flow uses `dash.turtle-flow.com` and a distinct API protocol.
- The customer dashboard’s current Ed25519 signing key ID is `tf-free-2026-1`; its public key must be embedded in the CLI before the CLI accepts leases signed by that key.

## 12. Accepted decisions

1. **Lease duration:** 30 days, followed by explicit `turtle login` and browser approval.
2. **Meaning of “login required”:** account linking is optional. Missing or expired account leases do not block local Free commands or data access.
3. **Installation slots:** two active CLI installations per account; revocation frees a slot immediately for future authorizations.
4. **Browser security step:** the existing dashboard account-protection requirement is sufficient; either a passkey or authenticator setup unlocks approval.
5. **Current free-license download:** it remains a generic account export. Only a signed lease bound to the installation public-key thumbprint counts as a linked CLI installation.
6. **License creation:** approving the first CLI authorization creates the account’s one free local license if the account has none; the page states this before approval.

## 13. Rollout sequence

1. Implement CLI local verification, OS credential-store key handling, `turtle login`, polling, and clear missing/expired/invalid-license messages.
2. Implement dashboard authorization tables, API endpoints, transactional two-slot enforcement, approval/denial, and device management UI.
3. Review and test expiration, duplicate polling, denial, slow-down, third-device races, device revocation, key rotation, offline expiry, and database outage.
4. Release CLI and dashboard support together; the CLI must fail closed for account-linked entitlements but fail open to local Free functionality.
