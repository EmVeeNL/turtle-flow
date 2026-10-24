# Final Architecture Review

| Field | Value |
|---|---|
| Document | ARCHITECTURE-REVIEW.md |
| Status | Review of package baseline 0.1.0 |
| Reviewers | Architecture, security, product (roles per brief §1) |

This review critically evaluates the documented design against the criteria in the brief, lists the ten most important architectural risks, records corrections made to the original requirements, and gives prioritized next steps.

---

## 1. Corrections and Challenges to the Original Brief

The brief was taken as a starting point, not a specification. The following deviations are deliberate:

| # | Brief said / implied | This design | Why |
|---|---|---|---|
| C1 | Release Planning → Release Approval → Release as stages of every job | Release is a **separate aggregate**; jobs end at Delivery (optionally `awaiting_release`) | Releases bundle many jobs, have their own failure/rollback modes ([WORKFLOW.md §1](WORKFLOW.md#1-overview)) |
| C2 | Pause as part of the lifecycle | **Holds** overlay (pause, budget wait, rate-limit wait); `blocked` explicit state | Preserves workflow position; fewer transitions |
| C3 | Small local LLM that becomes knowledgeable | **Retrieval-first** structured knowledge + FTS5; local LLM optional for summarization/embeddings; **no fine-tuning** | Models don't learn from use; knowledge must be invalidatable ([ADR-010](adr/ADR-010-project-memory-context-retrieval.md)) |
| C4 | Configurable workflows | Profiles **select** from a fixed graph; no user-defined states | Keeps safety invariants provable |
| C5 | Git worktrees for isolation | Worktrees isolate *changes only*; security via separate tiers | Common misconception; documented in every relevant doc |
| C6 | Agents "manage Git" | **Turtle Flow owns all Git writes**; agents only edit files | Deterministic lineage; policy enforcement |
| C7 | Estimate remaining provider quota | Quota treated as **unknown** unless exposed; rate limits learned empirically | No reliable source (A-03) |
| C8 | Delivery merges into protected branch | Local delivery **never moves a branch checked out in the user's working tree** (unless opted in and clean); PR mode preferred | Prevents corrupting the user's checkout |
| C9 | Parallel execution | Parallel **across jobs**, sequential **within a job** in MVP | One worktree per job; sub-worktrees later |
| C10 | Agent session continuity across stages | **One process per run**, continuity via artifacts | Avoids context bloat, anchoring, and unrecoverable session state |

## 2. Evaluation Against Review Criteria

| Criterion | Assessment | Rating |
|---|---|---|
| Implementation feasibility | All components use mature crates and well-understood patterns (outbox, leases, state machine). Hardest parts: adapters (external volatility) and recovery (breadth). Feasible with spikes first. | Good |
| Rust ecosystem suitability | Tokio/Axum/SQLx/Clap are mainstream; Git via CLI avoids libgit2 gaps; FTS5 bundled. No exotic dependencies. | Strong |
| Maintainability | Clear crate boundaries with dependency rules; pure `decide`; adapters isolated. Risk: 13 crates may feel heavy early — acceptable, merging is cheap. | Good |
| Security | Strong deterministic controls (approval binding, git policy, gates, redaction, hardened Git). Weakest point: same-user execution (Tier 0/1). Container tier planned. | Adequate for MVP posture; documented residuals |
| AI provider compatibility | Capability model handles heterogeneity; nothing assumes identical features. Dependent on headless modes remaining available. | Good, externally exposed |
| Token efficiency | Per-role context budgets, artifact excerpts, gate-log trimming, stable prefixes, cross-provider review without transcripts. Orchestration overhead target G6 (≤ 35%) is plausible but must be measured. | Good |
| Workflow reliability | Explicit transitions, loop limits, `blocked` instead of silent failure; exhaustive tests. | Strong |
| Human oversight | Hash-bound approvals, invalidation, flags; acceptance mandatory. Risk of approval fatigue. | Strong (UX-dependent) |
| Crash recovery | Outbox + probes + leases + 25 crash-injected boundaries. | Strong if discipline holds |
| Git safety | Main worktree untouched; CAS ref updates; hooks disabled; serialized mutations. | Strong |
| MVP complexity | Large but cut: no knowledge layer, no historical routing, no releases, no containers, no multi-user. Still the largest risk to timelines. | Medium risk |
| Future commercial scalability | Seams for Postgres, workers, RBAC, entitlements in place; licensing friendly. Dependent on provider ToS for shared use. | Good |

## 3. Top 10 Architectural Risks

| # | Risk | Severity | Likelihood | Impact | Mitigation | MVP relevance |
|---|---|---|---|---|---|---|
| 1 | **Approval bypass under same-user execution.** An agent (via injection) or rogue local process forges approvals by calling the local API/CLI or editing the DB. | Critical | Medium | Unreviewed code reaches protected branches; trust in product destroyed | Layers L1–L7 (hash echo, TTY/dashboard channels, sandbox network denial, peer-PID checks, keychain-backed HMAC, guard-time verification, audit chain); L8 container/OS-user isolation; security tests SEC-008 | **High** — L1–L7 required for MVP; L8 post-MVP |
| 2 | **Provider CLI contract volatility.** Flags, output schemas or headless behavior change; features removed. | High | High | Broken runs, wrong usage data, security flags silently ignored | Capability discovery, compat ranges, pure invocation planning with golden tests, nightly canaries, adapter hot-fix releases, multiple providers | **High** — spikes S-1/S-2 first |
| 3 | **Provider terms of service / commercial constraints** on automated orchestration of consumer subscriptions, concurrency, or shared server use. | High | Medium | Feature restrictions; legal exposure; server tier viability | Legal review (OQ-01), API-key/enterprise account paths, clear user responsibility, design not dependent on subscriptions | Medium — affects positioning and defaults |
| 4 | **Recovery and idempotency complexity underestimated.** Each new effect adds probe logic; gaps cause duplicate commits/spend or stuck jobs. | High | High | Data inconsistency, lost trust, support burden | Outbox pattern from day one, DoD requires probes + crash tests, failpoint harness, kill harness, `blocked` fallback | **High** — critical path |
| 5 | **Insufficient containment of prompt injection / malicious repos** due to provider sandbox gaps (reads of secrets, loopback network). | High | Medium | Secret exfiltration, credential theft | Spike S-4 matrix, network-off sandbox, never-include lists, env scrubbing, post-run checks, container tier for untrusted repos, honest posture docs | **High** — controls Tier 0/1 |
| 6 | **False confidence from AI review/verification.** Agents claim criteria met; review misses defects. | High | Medium | Defects delivered; users blame Turtle Flow | Deterministic gates authoritative; verification methods per criterion (prefer tests/commands); human acceptance always; reviewer independence; escape-rate metric M9 | Medium |
| 7 | **Token estimation inaccuracy and incomplete usage data.** Cold-start estimates too wide; providers report partial usage; overruns. | Medium | High | Budget friction, unexpected spend, noisy approvals | Provenance types, confidence labels, buffers, calibration loop, priors from eval suite, overrun handling, mid-run enforcement where streaming exists | Medium — heuristic only in MVP |
| 8 | **MVP scope too large** for the team; dashboard and adapters dilute focus on the critical path. | High | Medium | Delay; partially working features | Milestones with walking skeleton (M1); CLI-complete before dashboard; strict out-of-scope list; parallel workstreams | **High** |
| 9 | **Worktree environment cost.** Fresh worktrees require dependency installs; build caches cold; port collisions; disk usage. | Medium | High | Slow gates; flaky tests; poor UX | `worktree_setup`, shared caches per ecosystem, unique gate port bases, disk watchdog, cleanup policies, setup time shown in UI | Medium |
| 10 | **Approval fatigue and UX friction** undermine supervision (rubber-stamping) or drive users to bypass Turtle Flow. | Medium | High | Supervision becomes theater; churn | Show deltas since last approval, flags only when relevant, fast auto-scope for small `fast` jobs, keyboard approvals, batched notifications, measure approval latency (M6) | Medium |

Other risks tracked but not top-10: SQLite single-writer throughput (low likelihood, benchmark S-7); Windows demand (OQ-08); frontend supply chain (lockfiles, CSP); license cracking (accepted).

## 4. Recommended Next Steps (prioritized)

1. **Run spikes S-1 and S-2** (Claude Code & Codex headless contracts) and record fixture transcripts; amend [AGENT-PROTOCOL.md §9](AGENT-PROTOCOL.md#9-provider-specifics-and-verification-spikes) with verified facts.
2. **Run spike S-4** (sandbox efficacy on macOS and Linux) — results decide whether approval-bypass layer L3 holds and whether container isolation must move into the MVP.
3. **Obtain legal guidance on OQ-01** (provider ToS for orchestrated use) before public beta; it may change defaults (API keys vs. subscriptions).
4. **Create the Cargo workspace skeleton** with the 13 crates, dependency-direction check, CI (fmt, clippy, deny, tests on Linux/macOS).
5. **Implement `turtle-domain` + `turtle-store` schema + hash-chained events + outbox** (migration `0001_init.sql` from [DATA-MODEL.md](DATA-MODEL.md)).
6. **Implement `turtle-workflow::decide` with the full transition table and property tests** before any IO-heavy work; review the snapshot as a team.
7. **Build the effect dispatcher, leases, recovery framework and failpoint harness** with the first effect (`CreateWorktree`) to establish the pattern.
8. **Walking skeleton (M1)** with FakeAdapter: worktree → task → commit → gates → local squash delivery, end-to-end from CLI.
9. **Claude Code adapter + ledger/reservations (M2)**, then **approvals, review, verification, Codex (M3)**.
10. **Recruit 5–10 design-partner developers** for dogfooding from M2; instrument (locally) time-to-first-job, approval latency and estimate coverage.
11. Resolve OQ-02 (open-core vs. source-available) before the first public release; it affects repository structure and contributor policy.
12. ~~Decide the final product and binary name (OQ-07).~~ Resolved: **Turtle Flow**, binary `turtle`. Remaining: logo and trademark check before public launch.

## 5. Package Validation Report

Validation was performed by scripts over the generated package on 2026-10-24.

| Check | Method | Result |
|---|---|---|
| Relative Markdown links and anchors | Script resolving every `[text](path#anchor)` outside code blocks against files and GitHub-style heading slugs | 870 links checked, 0 errors (after fixing 9 anchor mismatches found in the first pass) |
| Mermaid diagrams | All 14 Mermaid blocks (12 required + 2 supplementary) rendered with `@mermaid-js/mermaid-cli` 11.4.2 using headless Chrome | 14/14 render; one initial failure (note attached to an undeclared state in the workflow diagram) fixed |
| Standalone diagram files | `diagrams/*.mmd` extracted from the `<!-- diagram: … -->` markers, so they match the documents | 12 files |
| Example files | JSON and NDJSON parsed with Python `json`; TOML with `tomllib`; YAML (workflow profile and job front matter) with the `yaml` npm CLI | All valid |
| Requirement references | Every `FR/NFR/SEC/UX/LIC-xxx`, `A-xx` and `OQ-xx` mentioned anywhere must be defined in the PRD | 0 undefined references |
| ADR structure | Each of the 18 ADRs contains Status, Context, Problem Statement, Decision, Considered Alternatives, Rationale, Consequences, Risks, Mitigations, Related ADRs | 18/18 complete |
| ADR ↔ PRD ↔ MVP alignment | Manual cross-check, recorded in [adr/README.md](adr/README.md#consistency-check-adr--prd--mvp); generated [traceability matrix](traceability/REQUIREMENTS-MATRIX.md) maps all 190 requirements to ADR, document, epic and test suite | No conflicts found. Requirements marked *Won't (MVP)* or post-MVP are mapped to their planned release |
| Dashboard mockups | 6 standalone 1920×1080 PNGs rendered from HTML sources; checked by eye for layout overflow and for data that stays consistent across screens (file counts, token totals, approval counts) | Pass after fixing an inconsistent diff summary, a project total that didn't add up, and a clipped panel |

**What was not validated:** none of the provider CLI behavior described in [AGENT-PROTOCOL.md §9](AGENT-PROTOCOL.md#9-provider-specifics-and-verification-spikes) has been checked against real CLI binaries. That is the purpose of spikes S-1 – S-5. The SQL DDL in [DATA-MODEL.md](DATA-MODEL.md) is an abridged proposal. It does execute cleanly in an in-memory SQLite 3.54 database (with FTS5 and triggers), but no queries or migrations have been run against it.
