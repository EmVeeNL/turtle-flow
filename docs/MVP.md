# MVP Definition — Turtle Flow 0.3 (Beta)

| Field | Value |
|---|---|
| Document | MVP.md |
| Status | Proposed baseline |
| Related | [PRD.md](PRD.md), [ROADMAP.md](ROADMAP.md), [TESTING-STRATEGY.md](TESTING-STRATEGY.md), [traceability/REQUIREMENTS-MATRIX.md](traceability/REQUIREMENTS-MATRIX.md) |

---

## 1. Purpose

The MVP delivers **one reliable end-to-end supervised development workflow** for a single developer on macOS or Linux: from a job description to a delivered, reviewed, tested change — with hash-bound approvals, budgets, full history, and crash recovery — using Claude Code CLI and OpenAI Codex CLI.

"Reliable" is defined operationally: the 25 crash-injection scenarios pass, the reference suite completes (AC-01), and no delivery is possible without a valid acceptance approval (AC-02).

## 2. MVP Scope

| Area | Included |
|---|---|
| Runtime | Single binary `turtle`; daemon + CLI client over Unix socket; one daemon per data dir |
| Persistence | SQLite (WAL), migrations, hash-chained event log, effect outbox, blob store |
| Projects | Register local repo, settings (commands, profile, budgets, protected paths), archive, project config file (tighten-only security) |
| Jobs | Create (CLI flags or Markdown file), priorities, pause/resume/cancel, dependencies (Should) |
| Workflow | Full job state machine; profiles `fast`, `standard`, `strict`; custom derived profiles (Should); holds; `blocked` resolution |
| Stages | Discovery (Should), specification, scope approval, planning, (plan approval in strict), implementation (sequential tasks), build & tests, review, verification, acceptance approval, delivery |
| Providers | Claude Code adapter, Codex adapter, FakeAdapter; capability discovery; rule-based routing with overrides; rate-limit backoff |
| Structured outputs | JSON schemas for discovery, spec, plan, review, verification; repair attempt |
| Git | Worktree per job, branch naming, Turtle Flow commits with trailers, agent git-policy check, conflict probe, delivery via local squash/ff/merge and PR (`gh`) |
| Gates | build, test, lint, format_check, custom commands; secret scan, protected paths, diff size, scope conformance, dependency changes (strict) |
| Approvals | Scope, plan (strict), budget increase, scope change, policy exception, acceptance; binding + invalidation; CLI TTY and dashboard channels; HMAC |
| Tokens | Usage records with provenance; budgets (global/project/job); reservations & settlement; heuristic preflight with priors; price table; API-equivalent labeling; basic analytics |
| Resources | Attach Markdown/text/code/JSON/YAML (PDF Should); versioning; trust levels; FTS5 retrieval; pinned resources |
| Context | Deterministic context packages with manifests, per-role budgets, redaction |
| Observability | Event history, transcripts, structured logs, `doctor`, `audit verify`, job export (Should) |
| Dashboard | Overview, Projects, Project detail, Job detail, Runs/logs, Approvals, Code review/diff, Token analytics (basic), Providers, Resources, Settings (view + basic edit) |
| Security | All SEC "Must" requirements ([SECURITY.md §9](SECURITY.md#9-critical-security-controls-required-for-the-mvp)) |

## 3. Out of Scope (MVP)

| Excluded | Reason | Planned |
|---|---|---|
| Local LLM inference, embeddings, vector search | Retrieval via FTS5 + structured facts is sufficient to start; adds model management complexity | 0.4–0.6 ([ADR-010](adr/ADR-010-project-memory-context-retrieval.md)) |
| Historical quantile estimation & scoring router | Needs data first; MVP collects it | 0.4 |
| Project knowledge layer beyond resources (facts, repo map, lessons) | Valuable but not needed for the first reliable loop | 0.4 |
| Release aggregate, deploy hooks, rollback automation | Delivery suffices for MVP; releases are separate aggregate | 0.5 |
| Fully autonomous production releases | Never by default (NG2) | — |
| Parallel tasks within one job | Needs sub-worktrees and merging | 0.6 |
| Multi-user collaboration, RBAC, SSO | Server phase | 2.x |
| Distributed workers, cloud orchestration | Server phase | 2.x |
| License files and enforcement | Free tier only in MVP; `Entitlements` abstraction present | 1.0 |
| Container isolation tier | Opt-in after spike S-4 | 0.5 |
| Windows-native support | Sandbox and process semantics differ | Post-MVP decision (OQ-08) |
| Non-GitHub PR creation | Push supported, PR via GitHub `gh` only | 0.6 |
| Image resources & image input | Capability-dependent | 0.5 |
| Optimization recommendations | Needs analytics history | 0.4 |
| Light theme | Dark-first | 0.5 |

## 4. Epics

| Epic | Title | Scope | Key requirements |
|---|---|---|---|
| **E01** | Foundation | Cargo workspace (13 crates), CI (Linux/macOS), lint/format, `cargo-deny`, config loading, logging, error codes | NFR-001, NFR-011, NFR-015, SEC-020 |
| **E02** | Persistence & event log | rusqlite/SQLite store, migrations, repositories, single writer, event hash chain, outbox, blob store | NFR-004, NFR-009, NFR-017, SEC-009 |
| **E03** | Workflow engine | States, transition table, guards, profiles, holds, loop limits, blocked resolution | FR-020 – FR-045 |
| **E04** | Daemon, scheduler & recovery | Daemon lifecycle, single-instance lock, scheduler, leases, limits, effect dispatcher, startup recovery | FR-037, FR-131, NFR-005, NFR-007, ADR-017 |
| **E05** | Process execution | `turtle-exec`: process groups, env allowlist, timeouts, output caps, rlimits, scratch dirs | SEC-004, SEC-007 |
| **E06** | Provider adapters | Trait, registry, Claude Code, Codex, FakeAdapter, routing (rules), structured output + repair, rate-limit handling | FR-050 – FR-066 |
| **E07** | Context & resources | Resource storage/versioning/trust, extraction & chunking, FTS5, context builder with manifests & budgets, secret redaction | FR-110 – FR-118, FR-061, SEC-003, SEC-010 |
| **E08** | Git subsystem | Worktrees, commits & trailers, git policy check, conflict probe, delivery modes, cleanup, recovery probes, hardening | FR-090 – FR-102, SEC-014 |
| **E09** | Quality gates | Command gates with parsers, static gates (secret, protected paths, diff size, scope, deps) | FR-028, SEC-011, SEC-016 |
| **E10** | Approvals & policy | Binding, invalidation, channels, HMAC, auto-scope rule (fast), policy precedence | FR-025, FR-032, SEC-008, SEC-015 |
| **E11** | Token engine | Usage capture, ledger, budgets, reservations, preflight (heuristic + priors), pricing, analytics queries | FR-070 – FR-086 |
| **E12** | CLI | All commands, `--json`, TTY approval flow, `init` wizard, `doctor`, `audit verify`, `export` | FR-130 – FR-136, UX-011 |
| **E13** | API & dashboard | Axum API, auth, SSE, OpenAPI, SPA with core screens | FR-138, FR-139, UX-001 – UX-030, SEC-001/002 |
| **E14** | Hardening & beta release | Security test suite, crash-injection completion, performance, docs, signed releases | AC-01 – AC-10 |

## 5. MVP User Stories (by epic)

Stories reference PRD user stories (`US-xx`) and add implementation-level stories (`MS-xx`).

| ID | Epic | Story | Acceptance criteria |
|---|---|---|---|
| MS-01 | E01 | As a developer, I install one binary and run `turtle --version` | Release artifacts for 4 targets; smoke test |
| MS-02 | E02 | As the system, every state change is persisted with events in one transaction | Kill-9 during writes never yields state without events or vice versa |
| MS-03 | E03 | As a developer, my job follows the profile's stages exactly | Transition table snapshot tests; profile matrix test |
| MS-04 | E04 | As a developer, after a reboot Turtle Flow resumes my jobs (US-12) | Crash-injection suite green |
| MS-05 | E04 | As a developer, two jobs run in parallel within limits (US-18) | Scheduler property tests; E2E-08 |
| MS-06 | E05 | As a developer, a runaway agent is terminated within its timeout | SEC-007 tests |
| MS-07 | E06 | As a developer, I see providers detected with versions and auth status (FR-052) | `turtle providers list` |
| MS-08 | E06 | As a developer, I can force Codex for implementation and Claude for review (US-15) | Overrides honored; routing decision recorded |
| MS-09 | E06 | As the system, invalid structured output is repaired once, then fails clearly | Fault-injection test |
| MS-10 | E07 | As a developer, I attach an OpenAPI file and only relevant chunks are sent (US-17) | Context manifest shows inclusion reasons; budget respected |
| MS-11 | E07 | As a developer, secrets in my repo never reach the provider | Secrets corpus test |
| MS-12 | E08 | As a developer, my main checkout is never modified (US-07) | Invariant test |
| MS-13 | E08 | As a developer, I can deliver as a PR (US-11) | E2E-11 |
| MS-14 | E09 | As a developer, failing tests are fixed automatically up to a limit (US-09) | E2E-02, E2E-03 |
| MS-15 | E10 | As a developer, I approve the spec I actually read (US-04, US-05) | Hash echo; invalidation on edit |
| MS-16 | E10 | As a developer, protected-path changes require my explicit acknowledgement | SEC-011 test |
| MS-17 | E11 | As a developer, I see an estimate range before approving scope (US-06) | Preflight report attached to approval |
| MS-18 | E11 | As a developer, a job never exceeds its hard budget without my approval (US-14) | E2E-06; ledger property tests |
| MS-19 | E11 | As a developer, I see usage per provider/model/project/job (US-16) | Analytics view + CLI |
| MS-20 | E12 | As a developer, I can do everything from the CLI with `--json` | Parity matrix |
| MS-21 | E13 | As a developer, I review diffs with AI findings inline (US-10) | Review screen E2E |
| MS-22 | E13 | As a developer, I watch agent output live (US-08) | SSE latency test |
| MS-23 | E14 | As a developer, I get from install to first scope approval in ≤ 10 min (G5) | Usability test n ≥ 5 |

## 6. Implementation Milestones

Milestones are dependency-ordered. Durations are ranges under the staffing assumptions in [ROADMAP.md §1](ROADMAP.md#1-phases-at-a-glance); they are not commitments.

| Milestone | Goal | Contents | Exit criteria | Range |
|---|---|---|---|---|
| **M0 — Spikes** | De-risk | S-1 … S-7 ([AGENT-PROTOCOL.md §9.3](AGENT-PROTOCOL.md#93-verification-spikes)) | Spike reports; fixture transcripts recorded; ADRs amended if needed | 2–4 wk |
| **M1 — Walking skeleton** | Full loop without AI | E01, E02, E03 (fast profile), E04 (basic scheduler + recovery framework), E05, E08 (worktree, commit, local squash), E09 (command gates), E12 (core commands), FakeAdapter | Scripted E2E-01 (fast) passes; crash tests for implemented effects | 6–8 wk |
| **M2 — First real agent** | Claude Code end-to-end on `fast` | Claude adapter, capability registry, structured output, context builder v1 (no resources), usage capture, ledger + job budgets | A real `fast` job delivered on ref-rust-cli; transcripts stored; usage with provenance | 3–4 wk |
| **M3 — Supervised standard workflow** | Approvals, review, verification, Codex | Codex adapter, routing rules + overrides, `standard` profile, approvals (binding, invalidation, HMAC, TTY channel), review & verification stages, preflight (heuristic), project/global budgets, rate-limit handling | AC-01 partially (CLI only); AC-02, AC-04 pass | 6–8 wk |
| **M4 — Dashboard core** | Visual oversight | API v1 + auth + SSE; screens: overview, projects, project detail, job detail, runs/logs, approvals, review/diff | Dashboard approval flow E2E; SEC-001/002 tests | 5–7 wk (parallel to M3 from API freeze) |
| **M5 — Completeness & safety** | Beta-complete feature set | `strict` profile, plan approval, PR delivery, conflict probe & refresh, resources + FTS + redaction, static gates, analytics screen, providers/resources/settings screens, full crash-injection suite, security test suite | AC-03, AC-05 – AC-09 pass | 6–8 wk |
| **M6 — Beta hardening** | Ship 0.3 | `init` wizard, `doctor`, export, performance work, docs, signed releases, live E2E on 3 reference repos, usability tests | AC-01 – AC-10 all pass | 3–5 wk |

### 6.1 Progress against the milestones (2026-10-25)

| Milestone | State |
|---|---|
| M0 spikes | S-1 partly and S-2 Codex read-only/structured and workspace-write success paths recorded; S-3 to S-7 not started |
| **M1 walking skeleton** | **Complete** (see the exit-criteria check below). |
| M2 first real agent | **Adapter, ledger, estimator, repository context builder v1, failed-run reported usage accounting, permission-denial events, redacted transcripts, run CLI and a first capability registry done; a live run on a real project remains open.** Every run has usage provenance; reported failed-run usage is charged and closes its reservation but does not train the estimator. Failed runs without usage remain unavailable and release their reservation. Denials are hash-chained `run.policy_denied` events. Runner streams are redacted and capped at 2 MiB before content-addressed storage; gate output is redacted before it is stored. `turtle runs list` and `turtle runs transcript` expose the records. The estimator learns from reported successful runs and retains the cold-start role estimate. Context manifests remain deterministic, redacted and linked to runs; per-role context budgets and resources/FTS/knowledge retrieval remain later work. Still open for M2: automatic preflight approval binding and retry-cost calibration, currency budgets and the price table, budget-increase approval (a held job resumes when the budget is raised), persistent capability snapshots, and a live run on `ref-rust-cli`. |
| M3 | Part 1 plus rate-limit handling: Codex runner, capability registry, ordered routing, review independence, decision records, provider-limit observations, reset-aware holds/deferral, active-limit exclusion, Claude quota snapshots, soft 90% quota preference and CLI visibility implemented. Broader provider contract tests remain. No real Claude limit transcript exists yet; the Claude limit path has synthetic tests. |
| M4 | In progress: the daemon serves a loopback Axum API, one-use dashboard login, authenticated approval decisions, job commands, project/job creation, token budget updates, SSE, and static Astro assets. Full and read-only sessions are enforced by the server; `turtle dashboard --phone` can issue a Tailscale-only QR login while the daemon remains loopback-bound. The real adapter now renders the mock screen structure for available records across overview, projects, jobs, runs, approvals, analytics, and providers. Settings and license are read-only; resources explain the unavailable data path. API tests cover the new writes and phone-session rejection. Playwright coverage is defined against the mock preview, but `npm run test:e2e` is blocked because the required Chromium headless shell is not installed; attempts to install or launch a browser are blocked by this macOS sandbox. No working Tailscale status/Serve environment was available; Serve flags were checked against current docs and CLI integration is tested with a stand-in. |
| M5 core | Strict jobs run discovery, request hash-bound plan approval and require independent review; review findings above the profile's autonomous fix severity block for a human decision. Standard plans above eight tasks also require approval. A moved local target is probed with `merge-tree`; clean jobs refresh and repeat gates, review, verification and acceptance, while conflicts block until a committed manual resolution. Projects may select `delivery = "pr"`; the job branch is pushed only after acceptance, `gh` opens a PR using the approved specification and approval hashes, and `turtle job sync` completes delivery after merge. Local squash remains the default. Static diff gates run with build/test/lint gates and persist their redacted result artifact, including acknowledgement flags bound into acceptance approvals. Resources are content-addressed and versioned; UTF-8 text/Markdown is redacted, chunked and indexed with bundled SQLite FTS5, and relevant project/job resources enter the bounded prompt context with trust labels. CLI and API resource search/read paths are available. Per-task strict gates, estimate-envelope plan threshold, PDF extraction, remote-base fetching, PR polling and the rest of M5 remain open. |
| M6 onboarding/package | `turtle init` is interactive and only registers the current repository after confirmation; build/test/lint detection is suggestion-only for Cargo, npm/pnpm, Python and Go. Non-TTY use refuses before opening Turtle Flow state. Job export is deterministic JSON or Markdown assembled from stored artifact blobs, hash-bound approvals, run usage provenance, events and delivery; transcript bodies remain referenced by hash. `job worktree` prints the deterministic path. `job cancel --delete-branch` queues idempotent worktree cleanup followed by deletion of the generated branch for the daemon. `job revert` creates a new chore job; after its normal scope and plan approvals, the existing idempotent worktree effect applies the recorded delivered commit with `git revert --no-edit` and probes its generated revert message on retry. The e2e crash suite checks that the factory revert commit is created exactly once across every scheduler boundary. Clap completions, install/checksum script, generated CLI help and install/quickstart/configuration/troubleshooting/security docs exist. The tag workflow builds embedded-dashboard binaries for macOS arm64/x86_64 and Linux x86_64/aarch64 and uploads checksummed archives to a draft release. There are no signing credentials; signing/notarization are documented manual steps. |

Rate-limit simplifications: an unknown reset is retried conservatively after one hour; the soft Claude quota threshold defaults to 90% and can be set with `TURTLE_CLAUDE_QUOTA_THRESHOLD` (a fraction from 0 to 1). Quota history is retained for the latest 100 samples per provider/window. Empirical concurrency learning is out of scope for M3 part 2.

M4 dashboard simplifications: analytics only aggregates token amounts with their stored provenance; the API has no usage timestamps or price table, so charts, currency cost and cost per verified job remain unknown. Provider screens show current limit observations and recorded quota windows; discovery, model management and routing edits are not available. Configuration provenance, licensing and resource ingestion are not exposed by the daemon, so those screens are read-only or show an explicit empty/unknown state. The project creator validates a canonical working Git checkout, commit, merge state and branch using the same checks as `turtle project add`; repository gate configuration is not editable in the dashboard.

**M1 exit criteria, checked.** "Scripted E2E-01 (fast) passes": `turtle-scheduler/tests/e2e.rs` runs whole jobs from backlog to delivered with real Git worktrees, gates, both approvals, fix loops and policy enforcement (the `fast` and `standard` profiles). "Crash tests for implemented effects": `turtle-scheduler/tests/crash.rs` and the e2e suite crash and restart at every named boundary, including every effect-executed boundary, and compare the result with an uninterrupted run. "E12 core commands": the `turtle` CLI drives the same loop (`turtle-flow/tests/cli.rs`, plus a hand-run job through a pseudo-terminal). Contents delivered: E01, E02, E03 (fast; standard also exists), E04 (scheduler, leases, recovery), E05, E08 (worktree, commit, local squash), E09 (command gates), E12 (core commands), FakeAdapter.

**Known M1 simplifications**, all deliberate and listed so they are not forgotten:
- gates run per job, not per task; tasks are tracked by commit trailers rather than the `tasks` table; the token ledger counts tokens processed (cache reads and writes included) and has no currency budgets yet;
- a local squash delivery parked for a checked-out branch still completes the job (the result waits at `refs/turtle/delivery/<key>`); hard pause cannot yet stop a running agent; automatic scope approval is off;
- no `turtle init`, job-file front matter or interactive approval queue (`turtle approvals` lists, `turtle approve <id>` decides); `job cancel` has no `--delete-branch`;
- the CLI still opens the database directly despite the M4 dashboard API; the daemon is exclusive per data directory through an OS file lock rather than a socket;
- the `kill -9` subprocess crash harness and power-loss simulation from TESTING-STRATEGY §6 are not written; crash testing is the in-process harness.

## 7. Technical Dependencies

| Dependency | Needed by | Risk | Mitigation |
|---|---|---|---|
| Git ≥ 2.38 (`merge-tree --write-tree`) | E08 | Low (2022 release) | `doctor` check; fallback: temporary worktree merge for probes |
| Claude Code CLI headless mode (A-01) | E06/M2 | Medium | Spike S-1, fixtures, compat ranges |
| Codex CLI `exec --json` + sandbox (A-02) | E06/M3 | Medium | Spike S-2 |
| `gh` CLI | PR delivery | Low | Optional; local delivery default |
| SQLite FTS5 (bundled via `libsqlite3-sys` `bundled` feature) | E07 | Low | Bundle SQLite |
| OS keychain (`keyring`) | E10 (HMAC key), dashboard session key | Medium on headless Linux | File fallback with warning |
| Rust crates: tokio, axum, rusqlite, clap, serde, jsonschema, schemars, utoipa, rust-embed, tracing, minijinja, nix, ulid, sha2, ed25519-dalek, fail, proptest, insta | All | Low | `cargo-deny`, pinned versions |
| Frontend: Astro, TypeScript, Tailwind CSS v4, Preact, `@fontsource-variable/jetbrains-mono` plus the Asap Sharp files in `docs/assets`, `astro-icon` + `@iconify-json/fluent`, Vitest, Playwright + axe-core | E13 | Low | Lockfile, embed at build |

## 8. Acceptance Criteria

The MVP is accepted when [PRD.md §27](PRD.md#27-acceptance-criteria-product-level) AC-01 – AC-10 pass. In addition:

- All requirements prioritized **Must** in the PRD are implemented and traced to passing tests in the [requirements matrix](traceability/REQUIREMENTS-MATRIX.md).
- No open defects of severity *critical* or *high*; known *medium* defects documented in release notes.
- Documentation: install, quickstart, CLI reference (generated), configuration reference, security model summary, troubleshooting.

## 9. Definition of Done

A work item (story/epic) is done when:

1. Code merged to `main` with review by another engineer.
2. Unit tests and relevant integration tests written and green on Linux and macOS.
3. **For any new effect:** idempotency key, completion probe, and crash-injection test exist.
4. **For any new state/transition:** transition table snapshot updated and property tests extended.
5. **For any new external input** (agent output, resource format, config field): validation and a negative/fuzz test.
6. **For any security control:** a regression test in TST-13.
7. Errors have stable codes and actionable messages (UX-012).
8. CLI and API parity for state-changing actions (UX-007); `--json` schema updated.
9. Events emitted for every state change with correct actor attribution.
10. Docs updated (config reference, CLI help, relevant design doc if behavior differs).
11. No new `clippy` warnings; `cargo deny` clean.

## 10. Risks (MVP-specific)

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| Provider CLI behavior differs from assumptions | Medium | High | M0 spikes before adapter work; compat ranges; FakeAdapter keeps the rest of the system testable |
| Recovery complexity underestimated | High | High | Recovery framework in M1, every effect crash-tested from day one (DoD #3) |
| Scope creep into "intelligence" features | Medium | Medium | Explicit out-of-scope list; collect data in MVP, use it in 0.4 |
| Dashboard consumes disproportionate effort | Medium | Medium | API-first; CLI complete in M3; dashboard screens prioritized (approvals and review first) |
| AI review/verification quality too low to be useful | Medium | Medium | Deterministic gates are authoritative; review advisory except blockers; human acceptance always |
| Approval bypass weakness on same-user execution | Medium | High | Layers L1–L7 in MVP; L8 (container) post-MVP; honest docs |
| Build/test setup in fresh worktrees too slow | Medium | Medium | `worktree_setup`, shared caches, guidance per ecosystem |

## 11. Testing Strategy (MVP summary)

All suites in [TESTING-STRATEGY.md](TESTING-STRATEGY.md) except TST-20 (evaluation suite, Could) are in MVP scope. Merge gates: TST-01–10, 12 (subset), 13–15, 17–18. Pre-release gates: all including live E2E (TST-16) and full crash-injection.

## 12. Suggested Development Order

1. **Spikes M0** — start S-1/S-2 immediately; they gate adapter design.
2. **`turtle-domain` types + `turtle-store` schema + event chain** — the foundation for everything.
3. **`turtle-workflow::decide` with the full transition table and tests** — before any IO, so the state machine is reviewable early.
4. **Effect outbox + dispatcher + recovery framework** — with the first effect (`CreateWorktree`) and its crash test, establishing the pattern.
5. **`turtle-exec` and `turtle-git`** — worktree, commit, policy check, local squash delivery.
6. **FakeAdapter + run supervisor** — exercise the whole loop (walking skeleton, M1).
7. **CLI core commands** — `up`, `project add`, `job create/show/watch`, `approve`.
8. **Claude Code adapter** (M2), then **ledger + reservations** (needed before real money is spent at scale).
9. **Approvals with binding/HMAC**, **review**, **verification**, **Codex adapter**, **routing rules** (M3).
10. **API + SSE + dashboard** approvals and review screens first, then job detail and overview (M4).
11. **Resources/FTS/redaction**, **static gates**, **PR delivery**, **strict profile**, **analytics** (M5).
12. **Hardening, onboarding, performance, docs, live E2E** (M6).

Parallelization guidance: one engineer on store/workflow/recovery (critical path), one on exec/git/gates, one on adapters/context/tokens; frontend engineer starts after API v1 draft (end of M2) using mock data from the OpenAPI document.
