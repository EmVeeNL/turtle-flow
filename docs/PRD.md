# Product Requirements Document — Turtle Flow

| Field | Value |
|---|---|
| Document | PRD.md |
| Version | 0.1.0 |
| Status | Proposed baseline |
| Owner | Product (Turtle Flow) |
| Last updated | 2026-10-24 |
| Related | [MVP.md](MVP.md), [ARCHITECTURE.md](ARCHITECTURE.md), [traceability/REQUIREMENTS-MATRIX.md](traceability/REQUIREMENTS-MATRIX.md) |

This PRD defines **what** the product must do and **how we will know it works**. Architectural decisions (the **how**) live in the [ADRs](adr/README.md) and technical documents. Requirements carry stable identifiers and MVP priorities (MoSCoW). "Won't (MVP)" means *not in the MVP*, not *never*.

---

## 1. Executive Summary

Turtle Flow is a local-first, CLI-first application written in Rust that turns locally installed AI coding agents — initially **Claude Code CLI** and **OpenAI Codex CLI** — into a supervised, reliable software-delivery pipeline. A developer describes a job ("add rate limiting to the public API"); Turtle Flow produces a specification, asks for scope approval, plans tasks, implements them in an isolated Git worktree using the most suitable agent, runs builds and tests deterministically, obtains an AI code review, auto-fixes minor issues within approved limits, verifies the result against acceptance criteria, asks for human acceptance, and delivers the change through a controlled Git workflow.

The differentiators are:

1. **Deterministic orchestration.** Rust owns state, policy, budgets, approvals and Git. Agents produce artifacts; they never decide what happens next.
2. **Hash-bound approvals.** Humans approve exact artifact revisions; approvals auto-invalidate when assumptions change.
3. **Token intelligence.** Every agent run is estimated, reserved against a budget, measured, and fed back into estimation and routing — with honest treatment of unknown quota.
4. **Recoverability.** Every step is persisted and idempotent; Turtle Flow survives crashes, reboots, rate limits and interrupted Git operations.
5. **Local-first.** One binary, one SQLite database, no account, no cloud dependency besides the AI providers the user already uses.

The MVP delivers **one reliable end-to-end workflow** for a single developer on macOS/Linux. Team collaboration, server deployment, advanced intelligence and licensing enforcement are designed for but deferred.

## 2. Product Vision

> *A developer can hand a well-scoped change to Turtle Flow, stay in control of every decision that matters, and get back a reviewed, tested, traceable branch — at a predictable token cost.*

Long-term, Turtle Flow becomes the **control plane for AI-assisted software delivery** in engineering teams: the place where AI work is scoped, budgeted, approved, executed, verified and audited, independent of which AI vendor does the work.

Vision principles:

- **Supervised autonomy, not unattended autonomy.** Autonomy expands only inside approved scope and budget.
- **Vendor neutrality.** Turtle Flow routes work to providers based on measured outcomes, not marketing.
- **Evidence over trust.** Every claim (tests passed, criteria met, tokens used) is backed by a recorded artifact.
- **Developer ergonomics first.** CLI parity for every action; the dashboard is for overview, review and approval.

## 3. Problem Statement

Developers increasingly use agentic coding CLIs, but using them for non-trivial work has recurring problems:

| # | Problem | Consequence today |
|---|---|---|
| P1 | Agent sessions are ad hoc; scope drifts mid-session | Large, unreviewable diffs; work the developer did not ask for |
| P2 | No durable state; a crash, rate limit, or closed terminal loses progress | Re-prompting, duplicated token spend |
| P3 | Quality checks depend on the agent "remembering" to run tests | Broken code delivered as "done" |
| P4 | Token/cost usage is opaque and only visible after the fact | Budget surprises; no basis for choosing cheaper models |
| P5 | Parallel agent sessions in one checkout collide | Corrupted working trees, lost changes |
| P6 | No audit trail linking a commit to the prompt, model, context and approval that produced it | Unacceptable for regulated or team settings |
| P7 | Choosing a provider/model is guesswork | Over-spending on simple tasks; under-powered models on hard ones |
| P8 | Project knowledge (conventions, past failures) is re-explained in every session | Wasted tokens and inconsistent output |

## 4. Target Users

| Segment | Description | MVP? |
|---|---|---|
| **Individual professional developers** | Use Claude Code and/or Codex daily; want structured, reliable delegation of medium-size tasks | **Primary MVP target** |
| **Technical leads in small teams (2–10)** | Want consistent AI workflows, budget control, review discipline | Post-MVP (Team tier) |
| **Engineering organizations (10–500)** | Need policy, audit, SSO, central budgets, self-hosted server | Phase 2 (Business/Enterprise) |
| **Consultancies / agencies** | Many client projects, strict separation and cost attribution | Post-MVP; validates multi-project design |

Non-target: non-developers who want "no-code app generation"; teams expecting fully unattended production deployment.

## 5. User Personas

**P-1 — Maya, Senior Backend Engineer (primary).** Works on a Rust/TypeScript monorepo. Uses Claude Code daily and Codex occasionally. Wants to delegate well-defined tickets while she does design work, but refuses to merge anything she has not seen pass tests. Pain: agents that "finish" with failing tests; losing an hour of agent work when her laptop sleeps. Success: queues three jobs in the morning, approves scopes in five minutes, reviews diffs after lunch.

**P-2 — Daniel, Tech Lead of a 6-person team (secondary, post-MVP).** Responsible for the team's AI spend and code quality. Wants per-project budgets, consistent review policy, and evidence that AI changes were reviewed. Success: monthly report of cost per verified job per project; no unreviewed AI code on `main`.

**P-3 — Aisha, Freelance Full-Stack Developer (secondary).** Maintains eight client projects with different stacks. Needs strict isolation of client resources and per-client cost attribution. Success: switching projects never leaks context; invoices can include AI cost line items.

**P-4 — Priya, Platform/Security Engineer at an enterprise (Phase 2).** Evaluates whether AI agents can run inside the company. Needs self-hosting, SSO, audit logs, sandboxing guarantees, and offline licensing. Success: passes the internal security review.

## 6. User Stories

Stories are referenced by MVP epics in [MVP.md](MVP.md). IDs: `US-xx`.

| ID | As a… | I want to… | So that… | Priority |
|---|---|---|---|---|
| US-01 | developer | register an existing local Git repository as a project with one command | I can start using Turtle Flow in under 10 minutes | Must |
| US-02 | developer | configure build, test and lint commands per project | quality gates run the same checks I run | Must |
| US-03 | developer | create a job from a title and description (CLI or dashboard) | I can delegate work quickly | Must |
| US-04 | developer | review and edit a generated specification before any code is written | the agent builds what I actually want | Must |
| US-05 | developer | approve, reject or request changes to the scope with a comment | I control scope | Must |
| US-06 | developer | see an estimated token usage range and budget impact before approving | I avoid budget surprises | Must |
| US-07 | developer | have implementation happen in an isolated worktree | my working checkout is untouched | Must |
| US-08 | developer | see live agent output and the current stage | I know what is happening | Must |
| US-09 | developer | have failing builds/tests automatically sent back for fixing up to a limit | I don't babysit trivial failures | Must |
| US-10 | developer | read an AI review with findings ranked by severity next to the diff | I can review efficiently | Must |
| US-11 | developer | accept a job and have it delivered as a merge or pull request | delivery is controlled and traceable | Must |
| US-12 | developer | have Turtle Flow resume correctly after a crash or reboot | I don't lose work or tokens | Must |
| US-13 | developer | pause, resume and cancel jobs | I stay in control | Must |
| US-14 | developer | set a per-job and per-project budget | spend is capped | Must |
| US-15 | developer | override provider/model for a job or task | I can apply my own judgment | Must |
| US-16 | developer | see token usage per provider, model, project and job | I understand where tokens go | Must |
| US-17 | developer | attach requirement documents and API specs to a job | agents have the right reference material without me pasting it | Must |
| US-18 | developer | run two jobs in the same project concurrently when they don't conflict | throughput improves safely | Should |
| US-19 | developer | see which provider/model performs best per task category on my projects | routing decisions are evidence-based | Should |
| US-20 | developer | have project conventions and past failures included automatically when relevant | I don't repeat myself | Should |
| US-21 | developer | export a job report (spec, plan, diff summary, gates, tokens, approvals) | I can share evidence | Should |
| US-22 | tech lead | define a strict profile requiring cross-provider review for security-sensitive paths | risky code gets extra scrutiny | Should |
| US-23 | developer | bundle delivered jobs into a release with notes and a tag after approval | releases are controlled | Could |
| US-24 | tech lead | invite teammates with roles and shared approvals | the team shares one installation | Won't (MVP) |
| US-25 | platform engineer | run Turtle Flow as a server with SSO and worker nodes | the organization can adopt it | Won't (MVP) |

## 7. Jobs to Be Done

| JTBD | Situation | Motivation | Expected outcome |
|---|---|---|---|
| J1 | When I have a well-understood ticket I don't want to type myself | delegate it to an AI agent safely | a tested branch I can review in minutes |
| J2 | When I'm about to let an agent change code | agree on scope first | no surprise changes outside scope |
| J3 | When agent work fails midway (crash, rate limit, test failure) | continue without starting over | no lost work, no double spend |
| J4 | When I choose which AI to use | pick the cheapest one that will succeed | lower cost per successful change |
| J5 | When someone asks "why is this code here?" | trace it to the job, approval, prompt and model | defensible audit trail |
| J6 | When I run multiple AI tasks at once | keep them from interfering | parallelism without corruption |

## 8. Product Goals

| ID | Goal | Measured by (see §23) |
|---|---|---|
| G1 | Reliable end-to-end supervised workflow | ≥ 80% of `standard` jobs in the reference evaluation suite reach `accepted` without manual code edits |
| G2 | Zero silent failures | 100% of terminal job states have a recorded cause; 0 jobs "lost" across crash-injection tests |
| G3 | Predictable cost | ≥ 80% of job-level actual token usage falls within the preflight P10–P90 range after 50 historical jobs per project |
| G4 | Trustworthy approvals | 0 cases where code is delivered without a valid acceptance approval bound to the delivered diff hash |
| G5 | Fast onboarding | Median time from install to first job in `scope_approval` ≤ 10 minutes |
| G6 | Low orchestration overhead | Turtle Flow-originated tokens (spec, plan, review, verify) ≤ 35% of total job tokens on `standard` profile, median |

## 9. Non-Goals

- **NG1:** Replacing the developer's IDE or the agents' own interactive UIs.
- **NG2:** Fully autonomous production deployment without human approval (never in default policy).
- **NG3:** Hosting or reselling AI model access. Turtle Flow uses the user's own provider accounts/CLIs.
- **NG4:** Training or fine-tuning models in the MVP.
- **NG5:** Project management features beyond jobs/tasks (no sprints, story points, Jira replacement).
- **NG6:** Guaranteeing security against a fully malicious repository in the MVP's default execution mode (see [SECURITY.md §3](SECURITY.md#3-security-posture-and-assumptions)).
- **NG7:** Windows-native support in the MVP (WSL2 is best-effort).
- **NG8:** Uncontrolled agent-to-agent chat. Agents collaborate only through versioned artifacts.

## 10. Functional Requirements

Acceptance criteria for each requirement are testable statements; IDs are referenced in the [traceability matrix](traceability/REQUIREMENTS-MATRIX.md).

### 10.1 Project Management (FR-001 – FR-012)

| ID | Requirement | Acceptance criteria | MVP |
|---|---|---|---|
| FR-001 | The system shall register a project from an existing local Git repository path. | `turtle project add <path>` succeeds for a repo with ≥ 1 commit; fails with a clear error for a non-repo, bare repo, or repo with unresolved merge state. | Must |
| FR-002 | Each project shall have an immutable ULID, a unique human slug, name, description, and creation timestamp. | Slug uniqueness enforced by DB constraint; renaming changes name, not ID or slug unless explicitly requested. | Must |
| FR-003 | Project settings shall be resolved from (lowest→highest precedence) Turtle Flow defaults, repository file `.turtle/project.toml`, and user-local overrides stored in Turtle Flow DB; repository files may only *tighten* security policy. | Unit tests show precedence; a repo file that attempts to relax `security.*` settings is ignored with a warning event (see SEC-015). | Must |
| FR-004 | The system shall store per-project commands for `build`, `test`, `lint`, `format_check` (each optional) and detect sensible defaults for Cargo, npm/pnpm, Go, Python (uv/pip), and Maven/Gradle. | Detection proposes commands; user confirms; commands persisted; gates use them verbatim. | Must (config) / Should (detection) |
| FR-005 | The system shall list, show, archive and unarchive projects. Deleting a project shall remove Turtle Flow data but never the Git repository. | Archive hides from defaults and blocks new jobs; delete requires typed confirmation; repo untouched (verified by test). | Must |
| FR-006 | Each project shall support agent preferences: allowed providers, preferred provider/model per role, disallowed models. | Router never selects a disallowed provider/model for that project (property test). | Should |
| FR-007 | Each project shall support budget configuration (see FR-072). | — | Must |
| FR-008 | Each project shall define a default workflow profile and path-based profile escalation rules (e.g. `src/auth/**` → `strict`). | Job touching escalated paths in its plan is escalated before implementation; escalation recorded as event. | Must (default) / Should (path rules) |
| FR-009 | `turtle doctor --project <slug>` shall verify repository state, configured commands, provider availability and authentication, disk space, and worktree health. | Each check reports pass/warn/fail with remediation text. | Should |
| FR-010 | The system shall support creating a project by cloning a remote URL. | Clone uses Git CLI with hooks disabled; credentials handled by Git's own helpers. | Could |
| FR-011 | Each project shall have an isolated data area for resources, artifacts, knowledge and transcripts. | Queries are always project-scoped; cross-project access requires explicit admin command (verified by repository-layer tests). | Must |
| FR-012 | The system shall display per-project summaries: active jobs, pending approvals, budget consumption, last delivery, gate pass rates. | Values consistent with underlying tables (integration test). | Must |

### 10.2 Jobs and Workflow (FR-020 – FR-045)

| ID | Requirement | Acceptance criteria | MVP |
|---|---|---|---|
| FR-020 | Users shall create jobs with title, description, type (`feature`, `bugfix`, `refactor`, `docs`, `chore`), profile, priority, attached resources, and optional budget override. | Job created in `backlog` (or `discovery` if `--start`); validation errors are explicit. | Must |
| FR-021 | Job state shall change only through the workflow engine according to the transition table in [WORKFLOW.md](WORKFLOW.md). | Any attempted invalid transition returns `TransitionRejected` with guard name; no DB row changes (property test over random event sequences). | Must |
| FR-022 | The system shall provide `fast`, `standard` and `strict` profiles; users may define custom profiles derived from them that enable/disable optional stages and adjust limits, but cannot add new states. | Profile validation rejects unknown stages and disabled mandatory stages (e.g. acceptance approval in `strict`). | Must (built-in) / Should (custom) |
| FR-023 | Discovery shall analyze the repository and job description and produce a discovery report (relevant files, risks, open questions, complexity signals). | Discovery report artifact exists with schema-valid JSON and Markdown rendering. | Should |
| FR-024 | Specification shall produce a versioned spec artifact containing: summary, in-scope, out-of-scope, acceptance criteria (each with ID and verification method), affected areas, risks, assumptions. | Schema validation passes; each acceptance criterion has `id`, `statement`, `verification` ∈ {`test`, `command`, `review`, `manual`}. | Must |
| FR-025 | Scope approval shall be required before planning/implementation in `standard` and `strict` profiles and shall bind to the spec revision hash, preflight estimate, and budget envelope. | Implementation cannot start without a `valid` scope approval (guard test); editing the spec invalidates it. | Must |
| FR-026 | Planning shall produce a plan artifact: ordered task DAG with per-task description, expected files/paths, acceptance-criteria mapping, role, complexity class, and estimated tokens. | Plan schema valid; DAG acyclic; every acceptance criterion mapped to ≥ 1 task or to verification. | Must |
| FR-027 | Implementation shall execute plan tasks in dependency order within the job worktree, one agent run at a time per job. | Two tasks of one job never run concurrently (scheduler invariant test). | Must |
| FR-028 | Build & test gates shall run configured commands in the worktree after each task (configurable: per task or per job) and record exit code, duration, truncated output, and parsed test summary where possible. | Gate results stored as artifacts; non-zero exit fails the gate. | Must |
| FR-029 | AI code review shall produce structured findings (`severity` ∈ {`blocker`,`major`,`minor`,`nit`}, file, line range, category, rationale, suggested fix) for the job diff against its base. | Review artifact schema-valid; findings reference lines present in the diff (validated; invalid refs dropped and counted). | Must |
| FR-030 | The system shall auto-fix gate failures and review findings of severity ≤ the profile's `auto_fix_max_severity` within `max_fix_iterations` and the job budget. | After N failed iterations the job transitions to `blocked` with reason `fix_limit_reached`, not an infinite loop. | Must |
| FR-031 | Verification shall evaluate each acceptance criterion using its declared method and produce a verification report with per-criterion status (`met`, `not_met`, `unverifiable`) and evidence links. | Report lists every criterion; `not_met` routes back per WORKFLOW rules. | Must |
| FR-032 | Acceptance approval shall bind to the final diff hash, verification report hash, and base commit. | Delivery guard refuses if the worktree HEAD diff hash ≠ approved hash. | Must |
| FR-033 | The workflow shall support controlled backward transitions (review→implementation, review→planning, verification→implementation/planning, scope change→specification). | Each backward transition increments the stage's loop counter and invalidates dependent approvals as specified. | Must |
| FR-034 | Users shall pause and resume jobs; pausing lets the current agent run finish (soft) or terminates it (hard). | After resume, the job continues from the persisted step; no duplicate commits (crash/idempotency tests). | Must |
| FR-035 | Users shall cancel jobs; cancellation terminates runs, releases reservations, preserves artifacts, and optionally removes the worktree and branch. | Cancelled jobs have zero open reservations and leases. | Must |
| FR-036 | Jobs shall support dependencies (`blocked_by`) within a project; a dependent job cannot start implementation until its dependencies are delivered. | Scheduler test. Cycles rejected. | Should |
| FR-037 | Multiple jobs per project may run concurrently, bounded by `project.max_concurrent_jobs` (default 2) and global `max_concurrent_runs` (default 3). | Limits never exceeded (scheduler property test). | Must |
| FR-038 | Each stage shall have a retry policy (max attempts, backoff, retryable error classes). | Retryable provider errors (rate limit, transient) retried; non-retryable errors (auth, schema violation after repair) fail fast. | Must |
| FR-039 | The system shall detect scope change: plan or implementation touching paths outside the approved spec's affected areas beyond a tolerance, or reviewer flagging `scope_creep`, triggers return to specification with renewed approval. | Test fixture with out-of-scope change triggers `scope_change_detected` event and approval invalidation. | Must |
| FR-040 | Jobs shall have priority (`low`, `normal`, `high`, `urgent`) influencing queue order. | Scheduler picks highest priority eligible job first; ties by creation time. | Should |
| FR-041 | When requesting changes, users shall be able to attach free-text feedback that is included in the next agent run's context package. | Feedback appears in the next context package manifest. | Must |
| FR-042 | Users shall be able to edit spec and plan artifacts directly (creating a new revision authored by the user). | New revision recorded with `author=user`; dependent approvals invalidated. | Must |
| FR-043 | The system shall enforce a maximum job wall-clock duration and per-stage timeouts from the profile. | Timeout transitions to `blocked` with reason. | Must |
| FR-044 | Jobs shall be creatable from a Markdown job file (see [examples/example-job.md](examples/example-job.md)). | `turtle job create --file job.md` parses front matter and body. | Should |
| FR-045 | The system shall support a "dry run" that performs preflight and specification only, without implementation. | Job ends in `scope_approval` and can be discarded without side effects on the repo. | Should |

### 10.3 AI Orchestration (FR-050 – FR-066)

| ID | Requirement | Acceptance criteria | MVP |
|---|---|---|---|
| FR-050 | The system shall provide a Claude Code CLI adapter supporting headless execution, streaming output, model selection, tool restriction, cancellation and usage capture as far as the installed CLI version supports. | Contract test suite passes against the pinned supported version range; unsupported features reported as capabilities = false. | Must |
| FR-051 | The system shall provide an OpenAI Codex CLI adapter with the same contract. | Same as FR-050. | Must |
| FR-052 | Adapters shall perform capability discovery: binary path, version, authentication status, available models (if discoverable), supported output formats, sandbox modes. | `turtle providers list` shows discovered capabilities; unknowns shown as `unknown`, never guessed. | Must |
| FR-053 | Adapters shall normalize provider output into a common `AgentEvent` stream (message, tool call, tool result, usage, error, completion). | Golden-file tests from recorded transcripts. | Must |
| FR-054 | Runs that must return structured data (spec, plan, review, verification) shall be validated against JSON Schemas; on validation failure, one repair attempt is made, then the run fails with `SchemaViolation`. | Fault-injection test with malformed output. | Must |
| FR-055 | Running agent processes shall be cancellable within 5 seconds (SIGTERM, then SIGKILL of the process group). | Cancellation test with a stubborn fake agent. | Must |
| FR-056 | Every run shall have a wall-clock timeout and, where supported, a max-turns limit. | Runs exceeding timeout are terminated and recorded as `timeout`. | Must |
| FR-057 | The system shall select provider and model by a rule-based policy (project preferences, role defaults, availability, budget) in the MVP, with historical scoring as an additional input when ≥ 20 runs exist for the bucket. | Router decision record stored for every run with inputs and reason. | Must (rules) / Should (scoring) |
| FR-058 | Users shall override provider/model per job, per task, or per role. | Overrides take precedence unless they violate policy (e.g. disallowed provider), which is an explicit error. | Must |
| FR-059 | Profiles may require that the reviewer provider/model differs from the implementer. | Strict profile default; router enforces. | Should |
| FR-060 | Adapters shall classify rate-limit errors, apply backoff honoring any provider-supplied retry hints, and optionally fail over to an allowed alternate provider. | Rate-limit simulation test. | Must (detect/backoff) / Should (failover) |
| FR-061 | Each run's context package shall be assembled deterministically, hashed, size-measured (estimated tokens), and stored with a manifest listing every included item and its source. | Manifest artifact present for 100% of runs. | Must |
| FR-062 | Full run transcripts (normalized events and raw output) shall be stored, subject to retention policy and secret redaction. | Transcript artifact retrievable from dashboard and CLI. | Must |
| FR-063 | New providers shall be addable by implementing the `ProviderAdapter` trait without changes to the workflow engine. | A test-only `FakeAdapter` and a second real adapter share all engine code paths. | Must (architecture) |
| FR-064 | The system shall support an evaluation command that runs a fixed task suite against selected providers/models and records comparable metrics. | `turtle eval run --suite core` produces a report. | Could |
| FR-065 | Agent runs shall never receive instructions to commit, push, or change branches; Turtle Flow performs all Git write operations. | Prompt templates and tool restrictions verified; post-run check detects unexpected commits/branch changes and fails the run (see FR-093). | Must |
| FR-066 | Agents collaborate only via artifacts: each run's inputs are artifacts/resources; outputs are artifacts. No direct agent-to-agent messaging. | Architecture constraint; no API exists for it. | Must |

### 10.4 Token Intelligence (FR-070 – FR-086)

| ID | Requirement | Acceptance criteria | MVP |
|---|---|---|---|
| FR-070 | The system shall record token usage per agent run with fields: input, cached input (read/write where available), output, reasoning (where available), total, and a **provenance** of `reported` (from provider output), `estimated` (computed by Turtle Flow), or `unavailable`. | 100% of completed runs have a usage record; provenance never `reported` unless parsed from provider output. | Must |
| FR-071 | Usage shall be aggregable by provider, model, project, job, task, role, stage, and time period. | Analytics queries return consistent totals across dimensions (integration test). | Must |
| FR-072 | Budgets shall be configurable at Turtle Flow, project and job level (tokens and/or currency, per period for Turtle Flow/project); task-level budgets are derived from plan estimates. | Exceeding a hard budget is impossible except via approved increase (property test). | Must (global/project/job) / Should (task) |
| FR-073 | Before each run, the system shall reserve estimated tokens (P90 + buffer) against all applicable budgets atomically, and settle to actuals after the run. | Ledger invariant: `committed + reserved ≤ limit` for hard budgets at all times. | Must |
| FR-074 | A job-level preflight shall compute complexity class, expected token range, recommended provider/model per role, budget impact, required reserve, and a decision (`allow`, `require_approval`, `deny`). | Preflight report artifact attached to scope approval. | Must |
| FR-075 | Estimation shall use historical quantiles per (project, role, complexity class, provider, model) bucket with hierarchical fallback to global priors. | After seeding history, estimates converge (offline evaluation). | Should |
| FR-076 | Estimates shall display a range (P10–P90) and a confidence level (`low`, `medium`, `high`) based on sample size and dispersion. | UI never shows a single estimate number without range and confidence. | Should |
| FR-077 | When a run's projected or actual usage would exceed a hard budget, the system shall stop before starting the run (or terminate mid-run where streaming usage is available) and request a budget-increase approval. | Job transitions to `awaiting_budget_approval`; no further runs start. | Must |
| FR-078 | The system shall maintain a user-editable price table per provider/model and compute cost; for subscription-based CLI usage it shall label cost as "API-equivalent" rather than actual spend. | Cost labels visible; price table versioned; historical costs use the price version in effect. | Must |
| FR-079 | The system shall compute cost and tokens per *verified* job (jobs that reached `accepted`), including tokens of failed attempts. | Metric shown per project and provider. | Should |
| FR-080 | Preflight shall estimate expected retry cost from historical fix-loop rates. | Included in preflight report. | Should |
| FR-081 | The system shall generate token optimization recommendations (e.g. oversized context items, models over-provisioned for task category). | Recommendations list with evidence. | Could |
| FR-082 | Each run shall record context package size (estimated input tokens by item category). | Visible in run detail. | Must |
| FR-083 | Provider quota and rate-limit information shall be stored with explicit status `known`, `partial`, or `unknown`; unknown values shall never be displayed as numbers. | UI test. | Must |
| FR-084 | Analytics retention shall be configurable (default: raw transcripts 90 days, usage records indefinitely). | Retention job prunes correctly. | Should |
| FR-085 | Users shall be warned when a project budget reaches configurable thresholds (default 50%, 80%, 100%). | Events and dashboard banners emitted. | Must |
| FR-086 | Token usage of internal AI calls (summarization, classification) shall be attributed like any other run. | No unattributed usage. | Must |

### 10.5 Git Integration (FR-090 – FR-102)

| ID | Requirement | Acceptance criteria | MVP |
|---|---|---|---|
| FR-090 | Each job entering implementation shall get a dedicated Git worktree under Turtle Flow data directory and a dedicated branch from the recorded base commit. | Worktree path and base SHA persisted before creation (idempotent). | Must |
| FR-091 | Branch names shall follow `turtle/<job-key>-<slug>` (configurable prefix), slug ≤ 40 chars, ASCII-safe. | Name validation tests. | Must |
| FR-092 | Turtle Flow shall create commits after each successful task with a conventional subject and trailers `Turtle-Job`, `Turtle-Task`, `Turtle-Run`, `Turtle-Provider`, `Turtle-Model`. | `git log` shows trailers; commit-to-run mapping in DB. | Must |
| FR-093 | After each agent run, Turtle Flow shall verify that HEAD, branch, and refs are unchanged by the agent (only working-tree changes allowed); violations fail the run and are recorded as security events. | Fixture agent that commits → run fails with `GitPolicyViolation`. | Must |
| FR-094 | Protected branches (default: the project's default branch plus configured patterns) shall never be checked out in Turtle Flow worktrees or written to except through the delivery step with a valid acceptance approval. | Guard tests. | Must |
| FR-095 | Delivery shall support (a) local merge into the target branch (fast-forward or squash, configurable) and (b) pushing the job branch and opening a pull request via `gh` (GitHub) when configured. | Both paths covered by integration tests (PR path with a fake `gh`). | Must (a) / Should (b) |
| FR-096 | Before delivery and periodically during long jobs, the system shall detect conflicts with the target branch using a non-mutating merge simulation (`git merge-tree --write-tree`). | Conflict produces `conflict_detected` event and routes per policy. | Must |
| FR-097 | The system shall refresh a job branch onto an updated base (rebase or merge, per policy) when no conflicts exist, re-running gates afterwards and invalidating acceptance approval. | Refresh test. | Should |
| FR-098 | Worktrees shall be removed after delivery or cancellation according to retention policy (default: delete worktree, keep branch 14 days). | Cleanup task test; `git worktree prune` executed. | Must |
| FR-099 | On startup and before Git operations, the system shall detect and recover interrupted Git states (stale `index.lock`, in-progress rebase/merge in a Turtle Flow worktree, missing worktree directories). | Crash-injection tests for each state. | Must |
| FR-100 | The system shall maintain a mapping of every Turtle Flow commit to job, task, run, approval and context package. | Query by commit SHA returns lineage. | Must |
| FR-101 | Turtle Flow Git commands shall run with repository hooks disabled and with a sanitized Git configuration. | `core.hooksPath` override verified (SEC-014). | Must |
| FR-102 | The system shall detect when the user's main working tree has uncommitted changes and never modify it. | Turtle Flow never writes to the main working tree (test). | Must |

### 10.6 Resource Management (FR-110 – FR-118)

| ID | Requirement | Acceptance criteria | MVP |
|---|---|---|---|
| FR-110 | Users shall attach resources (files or URLs captured as snapshots) to projects, jobs, or tasks. Supported in MVP: Markdown, plain text, source files, JSON/YAML (incl. OpenAPI). | Attach/detach via CLI and dashboard. | Must |
| FR-111 | Each resource version shall be stored content-addressed by SHA-256; re-attaching identical content creates no duplicate blob. | Dedup test. | Must |
| FR-112 | Resource metadata shall include title, kind, media type, size, hash, version, source (path/URL), trust level, tags, attached scopes, created/updated timestamps. | Schema test. | Must |
| FR-113 | Text shall be extracted and chunked (heading-aware for Markdown) for retrieval; PDF text extraction is supported. | Chunks indexed in FTS5. | Must (text/MD) / Should (PDF) |
| FR-114 | Resources shall be included in context packages only via relevance selection: explicitly pinned items first, then FTS-ranked chunks, within a per-run context budget. | Context manifest shows inclusion reason per item; total within budget. | Must |
| FR-115 | Each resource shall have a trust level (`trusted`, `internal`, `untrusted`); untrusted content is wrapped as data in prompts (see SEC-010). | Prompt template tests. | Must |
| FR-116 | Images (screenshots, diagrams) may be attached and passed to providers that support image input. | Capability-gated. | Could |
| FR-117 | OpenAPI resources shall support operation-level extraction (only relevant paths/schemas included). | Extraction test with fixture spec. | Could |
| FR-118 | External URL resources shall be fetched once on attach (snapshot), never fetched implicitly at run time. | No network access from context assembly (test). | Must |

### 10.7 Project Intelligence (FR-120 – FR-128)

| ID | Requirement | Acceptance criteria | MVP |
|---|---|---|---|
| FR-120 | The system shall maintain structured project facts (conventions, commands, architecture notes, decisions) with provenance (user, agent-proposed+approved, derived) and validity (commit or file-hash scope). | Facts CRUD and provenance displayed. | Should |
| FR-121 | The system shall generate a compact repository map (directory tree, key files, language stats; symbols where tree-sitter support exists) refreshed on base-commit change. | Map generated in < 10 s for 10k-file repo on reference machine. | Should |
| FR-122 | The system shall provide full-text search (SQLite FTS5) over resources, facts, artifacts and job history. | Search CLI and API. | Must (resources) / Should (others) |
| FR-123 | Knowledge items tied to files shall be marked stale when the referenced files' content hash changes. | Staleness test. | Should |
| FR-124 | The system shall record "lessons" from failed gates and resolved fixes (error signature → resolution summary) for future retrieval. | Lessons retrieved for matching error signatures. | Could |
| FR-125 | Agents may propose new facts; proposals require user approval before becoming active. | Proposal queue in dashboard. | Could |
| FR-126 | Embedding-based semantic search. | — | Won't (MVP) |
| FR-127 | Local LLM inference (summarization, reranking, classification). | — | Won't (MVP) |
| FR-128 | Model fine-tuning on project data. | — | Won't (MVP) |

### 10.8 CLI, Daemon and Observability (FR-130 – FR-140)

| ID | Requirement | Acceptance criteria | MVP |
|---|---|---|---|
| FR-130 | The CLI shall provide commands for: daemon (`up`, `down`, `status`), projects, jobs, approvals, runs, logs, budgets, providers, resources, search, doctor, export, config. | Command reference generated from Clap; each command has help and examples. | Must |
| FR-131 | `turtle up` shall start the daemon in the background if not running; CLI commands auto-start it when needed (configurable). Only one daemon per data directory may run. | Second daemon fails with `AlreadyRunning`. | Must |
| FR-132 | Every job shall have a complete, ordered event history viewable in CLI and dashboard. | `turtle job events <id>` output matches DB. | Must |
| FR-133 | Live agent output and state changes shall stream to CLI (`turtle job watch`) and dashboard (SSE). | Latency target NFR-003. | Must |
| FR-134 | All CLI read commands shall support `--json` output with a documented, versioned schema. | Snapshot tests. | Must |
| FR-135 | `turtle doctor` shall check environment: provider CLIs, Git version (≥ 2.38 for `merge-tree --write-tree`), disk, permissions, port, DB integrity. | Each check documented. | Must |
| FR-136 | Users shall export a job report (Markdown + JSON) with spec, plan, approvals, gate results, review summary, verification, token usage and commit list. | Export test. | Should |
| FR-137 | Desktop notifications for approvals needed and job completion. | OS notification on macOS/Linux. | Could |
| FR-138 | The dashboard shall provide all approval actions available in the CLI. | Parity test list. | Must |
| FR-139 | The system shall provide a local HTTP API (same as used by the dashboard) with a versioned OpenAPI description. | OpenAPI generated and published at `/api/v1/openapi.json`. | Should |
| FR-140 | Logs shall be structured (JSON lines) with rotation and include job/task/run correlation IDs. | Log format test. | Must |

### 10.9 Release (FR-150 – FR-156)

| ID | Requirement | Acceptance criteria | MVP |
|---|---|---|---|
| FR-150 | Users shall create a Release that bundles delivered jobs, with a version and target ref. | Release entity with state machine (see WORKFLOW §8). | Could |
| FR-151 | The system shall generate release notes from delivered jobs' specs and commits. | Release notes artifact. | Could |
| FR-152 | Release execution shall require a release approval bound to the release plan hash and target commit. | Guard test. | Could |
| FR-153 | Release execution shall create an annotated (optionally signed) Git tag. | Tag created only after approval. | Could |
| FR-154 | Release execution may run user-configured deploy hooks with timeouts and capture output. | — | Won't (MVP) |
| FR-155 | Release failure shall trigger a configurable rollback procedure. | — | Won't (MVP) |
| FR-156 | Production releases shall always require human approval regardless of profile. | Policy cannot disable (non-configurable). | Must (as invariant once releases exist) |

## 11. Non-Functional Requirements

| ID | Category | Requirement | Measure / test | MVP |
|---|---|---|---|---|
| NFR-001 | Distribution | Single statically linked (or minimally dynamically linked) executable for macOS (arm64, x86_64) and Linux (x86_64, arm64, glibc ≥ 2.31). | Release CI produces artifacts; smoke test on each target. | Must |
| NFR-002 | Startup | Daemon ready to serve API ≤ 1 s after start with a 1 GB database on reference machine (Apple M2 / 8-core x86, NVMe). | Benchmark. | Should |
| NFR-003 | Responsiveness | Dashboard first load ≤ 2 s (local); event-to-UI latency p95 ≤ 500 ms; CLI read commands p95 ≤ 200 ms. | Benchmarks. | Must |
| NFR-004 | Durability | No committed state lost on process crash or power loss (SQLite WAL, `synchronous=FULL` for ledger and approvals; `NORMAL` acceptable for logs). After restart, recovery completes ≤ 30 s and resumes eligible work. | Crash-injection suite (kill -9 at every step boundary). | Must |
| NFR-005 | Idempotency | Every side-effecting step (agent run, commit, merge, worktree creation, gate execution) is idempotent or detects prior completion. | Replay tests. | Must |
| NFR-006 | Efficiency | Daemon idle RSS ≤ 150 MB; idle CPU ≤ 1%. Excludes agent processes. | Measurement. | Should |
| NFR-007 | Concurrency | Support ≥ 4 concurrent agent runs and ≥ 10 queued jobs on reference machine without UI degradation. | Load test with fake adapters. | Must |
| NFR-008 | Fault isolation | Adapter panics or malformed provider output never crash the daemon; affected run fails with a classified error. | Fuzz tests on adapter parsers. | Must |
| NFR-009 | Upgradeability | DB migrations are forward-only, automatic, preceded by an automatic backup; daemon refuses to start on a DB from a newer version. | Migration tests across versions. | Must |
| NFR-010 | Offline | All features except provider calls and optional PR creation work offline. No telemetry by default. | Network-disabled test run with fake adapters. | Must |
| NFR-011 | Logging | Structured logs, rotation (default 10 × 20 MB), configurable level, correlation IDs. | — | Must |
| NFR-012 | Accessibility | Dashboard meets WCAG 2.2 AA (contrast, keyboard, focus, ARIA live regions for streaming logs). | axe-core CI + manual audit. | Should |
| NFR-013 | Testability | Workflow engine branch coverage ≥ 90%; every transition guard has unit tests; adapters have contract tests with recorded fixtures. | CI coverage report. | Must |
| NFR-014 | Portability of data | All data exportable (JSON + blobs); DB schema documented. | Export/import round-trip test. | Should |
| NFR-015 | Toolchain | Rust stable, MSRV pinned (proposal: latest stable minus 2 at project start); no nightly features. | CI. | Must |
| NFR-016 | Scalability (local) | Supports ≥ 50 projects, ≥ 10,000 jobs, ≥ 1,000,000 events, ≥ 50 GB blobs per installation without query p95 > 200 ms for dashboard views. | Synthetic data benchmark. | Should |
| NFR-017 | Determinism | Given the same event log, projections (job state, budgets) rebuild identically. | Rebuild test. | Must |
| NFR-018 | Time | All timestamps stored as UTC RFC 3339 with milliseconds; monotonic clocks for timeouts. | — | Must |

## 12. User Experience Requirements

| ID | Requirement | Measure | MVP |
|---|---|---|---|
| UX-001 | Dark-first visual theme with an equivalent light theme; theme follows OS by default. | Visual review. | Must (dark) / Should (light) |
| UX-002 | Every screen defines loading, empty, error, and permission-denied states. | Checklist in [DASHBOARD-UX.md](DASHBOARD-UX.md). | Must |
| UX-003 | Live data updates without page reload via SSE, with visible connection status and automatic reconnect + resync. | Disconnect test. | Must |
| UX-004 | Approval views show exactly what is being approved: artifact revision, hash prefix, diff/spec content, estimate, budget envelope, and what would invalidate the approval. | Usability review. | Must |
| UX-005 | Command palette (`⌘K`/`Ctrl+K`) and keyboard shortcuts for navigation and approval actions. | Keyboard-only walkthrough. | Should |
| UX-006 | Measured, estimated, and unknown values are visually distinct (solid / outlined-with-range / "unknown" badge), never interchangeable. | Visual tests. | Must |
| UX-007 | CLI parity: every state-changing dashboard action has a CLI equivalent. | Parity matrix. | Must |
| UX-008 | Layout optimized for ≥ 1280 px wide; usable down to 768 px; approvals usable on mobile widths (≥ 375 px) for remote access later. | Responsive tests. | Must (1280) / Should (768) |
| UX-009 | Text and UI contrast ≥ WCAG AA (4.5:1 text, 3:1 UI components). | Automated contrast checks on tokens. | Must |
| UX-010 | Destructive actions (cancel job with branch deletion, delete project) require explicit confirmation naming the object. | — | Must |
| UX-011 | Onboarding: `turtle init` interactive wizard detects providers, Git, and proposes project commands. | Time-to-first-job ≤ 10 min (G5). | Must |
| UX-012 | Errors are actionable: every error shows cause, affected object, and a suggested next action. | Error catalog review. | Must |
| UX-013 | Job timeline shows stages, loops (retries/back-transitions), approvals, gates and token usage in one view. | — | Must |

## 13. AI Orchestration Requirements (summary)

The AI orchestration requirements are FR-050 – FR-066 (§10.3). Cross-cutting principles:

1. **Agents are workers.** The engine decides which role runs next, with which context, on which provider, within which budget. Agents never trigger transitions directly; their outputs are evaluated by deterministic guards.
2. **Bounded runs.** Each run has a role, a context package, a budget reservation, a timeout, a max-turns limit (if supported), a tool policy, and a declared output contract.
3. **Structured outputs where the engine consumes them.** Spec, plan, review and verification outputs are JSON validated against versioned schemas; free text is allowed only for human-facing summaries.
4. **Artifact-mediated collaboration.** A reviewer sees the spec, plan and diff artifacts — not the implementer's conversation. This prevents shared hallucination and saves tokens.
5. **Honest capability handling.** Missing provider capabilities (e.g. no usage reporting) degrade explicitly (e.g. usage provenance = `estimated`).

Details: [AGENT-PROTOCOL.md](AGENT-PROTOCOL.md), [ADR-007](adr/ADR-007-provider-adapter-architecture.md), [ADR-018](adr/ADR-018-model-routing-and-evaluation.md).

## 14. Workflow Requirements (summary)

Workflow requirements are FR-020 – FR-045. The full state machine, guards, retry policies, and recovery behavior are specified in [WORKFLOW.md](WORKFLOW.md) and decided in [ADR-005](adr/ADR-005-workflow-state-machine.md). Profile defaults:

| Aspect | `fast` | `standard` | `strict` |
|---|---|---|---|
| Discovery | skip | optional (auto for complexity ≥ M) | required |
| Specification | lightweight (auto) | full | full + risk analysis |
| Scope approval | auto if preflight `allow` and complexity ≤ S | required | required |
| Planning | single-task plan | full DAG | full DAG + reviewer critique of plan |
| Gates | build + tests | build + tests + lint | build + tests + lint + format + custom |
| Review | same-provider, blockers only | full | full, cross-provider required |
| Auto-fix max severity | `major` | `minor` | `nit` |
| Max fix iterations | 2 | 3 | 3 |
| Verification | tests only | all criteria | all criteria + reviewer sign-off |
| Acceptance approval | required | required | required (+ second approver when multi-user, post-MVP) |
| Delivery | merge or PR | merge or PR | PR required |

Acceptance approval is **always required** for delivery to a protected branch in every built-in profile (decision; see [ADR-006](adr/ADR-006-supervised-autonomy-approvals.md)). A `fast` profile can deliver to a non-protected branch without acceptance only if a custom profile explicitly enables `auto_accept` — which is disabled by default and logged.

## 15. Project Management Requirements (summary)

FR-001 – FR-012. Isolation model: each project has its own repository, worktree root, budgets, resources, knowledge, artifacts, and concurrency limits. Provider *credentials* are not per-project in the MVP because the CLIs manage their own authentication globally per OS user — this is a documented limitation (see [SECURITY.md §7](SECURITY.md#7-credential-management) and OQ-03).

## 16. Token Intelligence Requirements (summary)

FR-070 – FR-086. The six-way distinction is mandatory in data model, API and UI:

| Concept | Source | Displayed as |
|---|---|---|
| Measured usage | Parsed from provider output (`reported`) | Exact number |
| Estimated usage | Turtle Flow estimator (pre-run) or tokenizer approximation (post-run when provider does not report) | Range or "≈" with provenance badge |
| User budget | User configuration | Limit bar |
| Provider rate limit | Provider error responses / headers if exposed | "Rate limited until …" or "unknown" |
| Provider account quota | Only if provider exposes it | "unknown" by default |
| Unknown capacity | Absence of data | "unknown" badge, never a number |

Details: [TOKEN-INTELLIGENCE.md](TOKEN-INTELLIGENCE.md), [ADR-008](adr/ADR-008-token-intelligence-budgets.md).

## 17. Security Requirements

| ID | Requirement | Acceptance criteria | MVP |
|---|---|---|---|
| SEC-001 | The HTTP server shall bind to `127.0.0.1` (and `::1`) by default; binding to other interfaces requires TLS and authentication configuration and is unavailable in the MVP. | Startup refuses non-loopback bind in MVP builds. | Must |
| SEC-002 | The dashboard/API shall require a per-installation bearer token (delivered via `turtle dashboard` opening a URL with a one-time login code exchanged for an HttpOnly, SameSite=Strict cookie) and validate `Host` and `Origin` headers to prevent DNS rebinding and CSRF. | Security tests with forged Host/Origin. | Must |
| SEC-003 | Context packages shall be scanned for secrets (pattern + entropy based) and matches redacted before sending; `.env*`, key files and configured globs are never included. | Test corpus of secrets; 0 leaks in fixtures. | Must |
| SEC-004 | Agent and gate processes shall run with a scrubbed environment (allowlist), no Turtle Flow tokens, and a per-run temporary `HOME` overlay where feasible without breaking provider authentication. | Env inspection test. | Must |
| SEC-005 | Agent runs shall use provider-native sandbox/permission modes restricting writes to the worktree; modes that disable sandboxing/permissions require an explicit per-project policy and a recorded approval. | Adapter tests assert flags. | Must |
| SEC-006 | Tool policies per role shall restrict agent capabilities (e.g. reviewer: read-only; implementer: edit + allowed commands). | Adapter maps policy to CLI flags; unsupported restrictions reported. | Must |
| SEC-007 | All spawned processes shall run in their own process group with timeouts, output size limits, and (Linux/macOS) resource limits on CPU time and file size where supported. | Fork-bomb/large-output fixtures handled. | Must |
| SEC-008 | Approvals shall be bound to content hashes and invalidated per [WORKFLOW.md §6](WORKFLOW.md#6-approvals). Approvals can only be granted through authenticated user channels (CLI on a TTY or authenticated dashboard), never by agent output. | Bypass test suite. | Must |
| SEC-009 | The event log shall be hash-chained (each event stores SHA-256 of previous event + its own canonical content); `turtle audit verify` detects tampering. | Tamper test. | Must |
| SEC-010 | Untrusted content (repository files, resources marked untrusted, tool output, web snapshots) shall be delimited as data in prompts with explicit instructions not to follow embedded instructions; deterministic guards (not prompts) enforce policy. | Prompt-injection fixture suite (see TESTING-STRATEGY). | Must |
| SEC-011 | Diffs touching protected paths (default: CI configuration, `.turtle/`, `.github/`, build scripts, lockfiles, Dockerfiles, auth code patterns) shall be flagged and require explicit acknowledgement in acceptance approval. | Policy test. | Must |
| SEC-012 | Turtle Flow shall not store provider credentials; it relies on each CLI's own authentication. Any Turtle Flow-held secrets (license keys, future API tokens) are stored in the OS keychain. | Code review + test. | Must |
| SEC-013 | The data directory shall be created with `0700` permissions; DB and blob files `0600`. | Permission test. | Must |
| SEC-014 | Git operations shall run with hooks disabled (`-c core.hooksPath=/dev/null`), `protocol.file.allow` restricted, and no execution of repository-defined filters/`fsmonitor` (`-c core.fsmonitor=false`, filters left to user config only for checkout of trusted repos — see GIT-STRATEGY). | Malicious-repo fixture tests. | Must |
| SEC-015 | Repository-supplied Turtle Flow configuration may tighten but not relax security policy unless the user explicitly confirms the relaxation (recorded). | Precedence test. | Must |
| SEC-016 | Dependency manifest changes (new or upgraded dependencies) shall be flagged in review and acceptance; `strict` requires explicit approval. | Policy test. | Should |
| SEC-017 | License files shall be verified offline with Ed25519 signatures; verification failure degrades to Free features, never blocks data access. | Unit tests. | Could |
| SEC-018 | Optional container-based isolation (Docker/Podman) for gate commands and, where possible, agent runs. | Opt-in config. | Could |
| SEC-019 | Network egress restriction for gate commands (deny by default in strict profile when container isolation is enabled). | — | Won't (MVP) |
| SEC-020 | Turtle Flow releases shall be signed, ship SBOMs, and pass `cargo-deny` (licenses, advisories) and `cargo-audit` in CI. | CI. | Must |

Threat model and controls: [SECURITY.md](SECURITY.md).

## 18. Git Integration Requirements (summary)

FR-090 – FR-102. Key invariants: Turtle Flow never writes to the user's main working tree; agents never commit; protected branches are written only by the delivery step with a valid acceptance approval. See [GIT-STRATEGY.md](GIT-STRATEGY.md) and [ADR-009](adr/ADR-009-git-worktree-isolation.md).

## 19. Resource Management Requirements (summary)

FR-110 – FR-118. Resources are inputs, versioned and content-addressed, with trust levels and relevance-based inclusion. See [ADR-013](adr/ADR-013-resource-artifact-management.md).

## 20. Dashboard Requirements

| ID | Screen | Requirement | MVP |
|---|---|---|---|
| UX-020 | Main Dashboard | Overview of active jobs, pending approvals, budget status, provider health, recent events. | Must |
| UX-021 | Project Overview | List/grid of projects with health, active jobs, spend. | Must |
| UX-022 | Project Detail | Jobs, settings summary, budgets, resources, knowledge, gate pass rates. | Must |
| UX-023 | Job Detail & Timeline | Stage timeline with loops, tasks, runs, approvals, gates, tokens. | Must |
| UX-024 | Agent Runs & Logs | Live and historical transcripts, context manifest, usage. | Must |
| UX-025 | Approvals | Queue with bound artifacts, diffs, estimates; approve/reject/request changes. | Must |
| UX-026 | Code Review & Diff | Side-by-side/unified diff with inline AI findings. | Must |
| UX-027 | Token Analytics | Usage/cost by dimension, estimate accuracy, cost per verified job. | Must (basic) / Should (accuracy charts) |
| UX-028 | Providers & Models | Capability registry, health, routing preferences, price table. | Must |
| UX-029 | Resources | Upload, versions, attachments, trust levels, extraction preview. | Must |
| UX-030 | Settings & Policies | Profiles, gates, protected paths/branches, security policies. | Must (view + basic edit) |
| UX-031 | Licensing | Current tier, entitlements, license import. | Could |

Screen specifications: [DASHBOARD-UX.md](DASHBOARD-UX.md).

## 21. Licensing Requirements

| ID | Requirement | MVP |
|---|---|---|
| LIC-001 | The Free tier shall work without account, registration, network access, or license file. | Must |
| LIC-002 | Feature gating shall be implemented through a single entitlement service (`Entitlements::check(Feature)`) so tiers can be introduced without scattering checks. | Must (architecture, all features entitled in MVP) |
| LIC-003 | Paid tiers shall be activated via an offline-verifiable signed license file; online activation is optional. | Could |
| LIC-004 | License expiry or invalidity shall never block access to existing data, history or exports; it only disables paid features after a grace period (default 30 days). | Must (principle) |
| LIC-005 | No telemetry is required for licensing; optional, opt-in usage reporting may exist for Business/Enterprise seat reconciliation. | Must |
| LIC-006 | AI provider subscription costs shall be clearly separated from Turtle Flow license costs in UI and documentation. | Must |

Commercial model: [LICENSING.md](LICENSING.md).

## 22. Server Deployment Roadmap (summary)

Phase 2 introduces a separate **Turtle Flow Server** distribution reusing the core crates, with PostgreSQL, OIDC authentication, RBAC, organization management, and **worker nodes** that execute agent runs and gates and report back via leases over HTTPS. Server deployment targets Team (small self-hosted), Business and Enterprise tiers. See [ROADMAP.md](ROADMAP.md) and [ADR-015](adr/ADR-015-future-server-deployment.md).

MVP constraints that keep this path open:

- Domain and workflow crates have no dependency on SQLite-specific types (repository traits).
- All state changes go through the same command API the server will expose.
- Actor identity (`actor_id`, `actor_kind`) is recorded on every event from day one, even though the MVP has a single local user.
- Agent execution goes through an `Executor` abstraction that can be local or remote.

## 23. Success Metrics

| Metric | Definition | MVP target | Measured from |
|---|---|---|---|
| M1 Verified completion rate | Jobs reaching `accepted` ÷ jobs reaching `scope_approved` (excluding user cancellations) | ≥ 70% on reference suite; ≥ 60% in dogfooding | Event log |
| M2 Manual-edit rate | Accepted jobs where the user edited code in the worktree before acceptance | ≤ 30% | Commit authorship in worktree |
| M3 Estimate calibration | Share of jobs with actual tokens within P10–P90 | ≥ 80% after 50 jobs/project | Ledger + preflight |
| M4 Cost per verified job | Total tokens (all attempts) ÷ accepted jobs, per profile | Tracked; trend ↓ 20% over 3 months via routing | Ledger |
| M5 Recovery success | Crash-injection scenarios recovering without manual intervention | 100% of defined scenarios | Test suite |
| M6 Approval latency | Median time from approval request to decision | Reported (no target; UX signal) | Events |
| M7 Time to first job | Install → first job in `scope_approval` | ≤ 10 min median (n ≥ 10 test users) | Usability study |
| M8 Fix-loop efficiency | Average fix iterations per accepted job | ≤ 1.5 on `standard` | Events |
| M9 Gate escape rate | Accepted jobs later reverted or hotfixed within 14 days (user-reported tag) | ≤ 10% | User tagging |
| M10 Activation/retention (commercial) | Weekly active factories with ≥ 3 jobs/week | Tracked post-launch (opt-in telemetry only) | Opt-in |

## 24. Risks and Dependencies

| ID | Risk / dependency | Impact | Mitigation |
|---|---|---|---|
| R-01 | Provider CLIs change flags/output formats without notice | Adapter breakage | Version pinning ranges, capability probing, golden-file contract tests in CI against latest CLI nightly, fast adapter releases ([ADR-007](adr/ADR-007-provider-adapter-architecture.md)) |
| R-02 | Provider terms of service may restrict automated/headless orchestration of subscription plans | Legal/commercial blocker | Legal review per provider; support API-key auth paths; document user responsibility (OQ-01) |
| R-03 | Usage reporting incomplete or absent in some modes | Token intelligence accuracy | Provenance labeling; tokenizer-based estimation fallback |
| R-04 | Same-user execution means a compromised agent could tamper with Turtle Flow state | Approval bypass | Hash-chained audit, sandbox denies data dir, OS-user/container isolation as opt-in, honest documentation ([ADR-012](adr/ADR-012-agent-execution-security.md)) |
| R-05 | AI review quality is inconsistent | False confidence | Deterministic gates first; review findings are advisory except blockers; human acceptance always required |
| R-06 | Estimation inaccurate early (cold start) | Budget friction | Wide ranges with explicit low confidence; generous default buffers; priors from evaluation suite |
| R-07 | Scope of MVP too large for team | Delay | Strict MVP cut in [MVP.md](MVP.md); vertical slice first |
| R-08 | Git edge cases (submodules, LFS, huge repos, sparse checkouts) | Failures | Detect and refuse unsupported features in MVP with clear messages; LFS supported via Git CLI if installed |
| R-09 | Rate limits throttle throughput | Slow jobs | Per-provider concurrency limits; backoff; failover |
| D-01 | Git ≥ 2.38 installed | Required for `merge-tree --write-tree` | `doctor` check |
| D-02 | At least one supported provider CLI installed and authenticated | Required | Onboarding wizard |
| D-03 | Project builds/tests runnable non-interactively | Required for gates | Command detection + validation run |

Full architectural risk analysis: [ARCHITECTURE-REVIEW.md](ARCHITECTURE-REVIEW.md).

## 25. Assumptions

| ID | Assumption | Validation |
|---|---|---|
| A-01 | Claude Code CLI supports non-interactive execution with streamed JSON output (`-p` / `--output-format stream-json`) including a final result event with usage data. | Spike S-1 ([AGENT-PROTOCOL.md §9](AGENT-PROTOCOL.md#9-provider-specifics-and-verification-spikes)) |
| A-02 | Codex CLI supports non-interactive execution (`codex exec`) with JSON event output including token usage per turn, and sandbox modes restricting writes to the working directory. | Spike S-2 |
| A-03 | ~~Neither CLI exposes remaining subscription quota.~~ **Partly falsified (spike 2026-10-24):** Claude Code 2.1.310 emits `rate_limit_event` with per-window utilization (5-hour and 7-day) and reset times; Codex 0.163.0 only reports an unstructured "try again at <time>" error. Neither exposes absolute remaining tokens. | [Spike S-1/S-2 report](spikes/S-1-S-2-provider-cli-report.md) |
| A-04 | Users run Turtle Flow on a machine where they already trust the repositories they work on roughly as much as running their build locally. | Product positioning; documented in SECURITY |
| A-05 | Typical MVP jobs change ≤ 1,500 lines across ≤ 30 files; larger work is decomposed into multiple jobs. | Dogfooding data |
| A-06 | A single machine can run 3–4 concurrent agent runs before provider rate limits, not local resources, become the bottleneck. | Load test |
| A-07 | SQLite in WAL mode with a single writer task sustains Turtle Flow write load (< 200 writes/s peak). | Benchmark |
| A-08 | Developers accept a CLI-first tool with a local web dashboard (no native desktop app). | User interviews |
| A-09 | Provider-native sandboxes (Codex Seatbelt/Landlock modes, Claude Code permission rules/sandbox) are sufficient for the MVP's threat assumptions. | Security spike S-4 |

## 26. Open Questions

| ID | Question | Owner | Needed by |
|---|---|---|---|
| OQ-01 | Do provider terms permit orchestrated headless use under consumer subscription plans, and at what concurrency? | Product/Legal | Before public beta |
| OQ-02 | ~~Should the Free tier be open source (e.g. Apache-2.0 core) with paid closed-source extensions, or source-available for everything?~~ **Decided 2026-10-25: the source stays private (proprietary)**; see [LICENSING.md §8](LICENSING.md#8-open-source--source-availability-strategy-oq-02). The free tier ships as a free binary of the proprietary product; open: its end-user license terms. | Product/Business | Before public launch |
| OQ-03 | Can per-project provider credentials be supported by pointing CLIs at isolated config directories (e.g. `CLAUDE_CONFIG_DIR`, `CODEX_HOME`) without breaking authentication? | Engineering | Post-MVP (Team) |
| OQ-04 | What is the right default for review: same-provider (cheaper) or cross-provider (more independent)? Requires evaluation data. | Engineering | MVP beta |
| OQ-05 | Should spec/plan structured output be generated by the agent CLIs or by direct API calls (cheaper, more controllable) when the user has API keys? | Architecture | Post-MVP |
| OQ-06 | Which deploy integrations (GitHub Releases, container registries, Kubernetes) are most demanded for the Release feature? | Product | Post-MVP |
| OQ-07 | ~~Final product and binary name.~~ **Resolved 2026-10-24:** product **Turtle Flow**, binary `turtle` (free on PATH/Homebrew; crates.io package `turtle-flow` with `[[bin]] name = "turtle"` because the `turtle` crate name is taken). Data dir `~/.turtle`, repo config `.turtle/`, branch prefix `turtle/`. | Product | Done |
| OQ-08 | Do we need Windows-native support for the Team tier, or is WSL2 acceptable? | Product | Post-MVP |
| OQ-09 | Should the Free tier include the local HTTP API for scripting, or is that a paid "automation" feature? (Current proposal: included.) | Product | Before launch |

## 27. Acceptance Criteria (product level)

The MVP is accepted when all of the following hold (each maps to tests in [TESTING-STRATEGY.md](TESTING-STRATEGY.md)):

1. **AC-01 End-to-end:** On each of three reference repositories (Rust CLI, TypeScript web API, Python library), a `standard` job from the reference suite runs from creation to delivery with only the two required human approvals, using Claude Code for at least one and Codex for at least one.
2. **AC-02 Approval integrity:** Automated bypass tests confirm no delivery occurs without a valid acceptance approval bound to the exact diff hash; editing the spec after approval invalidates the scope approval.
3. **AC-03 Recovery:** Kill-9 of the daemon at each of the 25 defined step boundaries ([WORKFLOW.md §10](WORKFLOW.md#10-recovery-behavior)) results in correct resumption without duplicate commits, duplicate settlement, or orphaned worktrees.
4. **AC-04 Budgets:** A job whose projected usage exceeds its hard budget pauses in `awaiting_budget_approval`; no run starts beyond the limit.
5. **AC-05 Token honesty:** Usage records carry correct provenance; UI shows "unknown" for unavailable quota.
6. **AC-06 Gates:** Failing tests route the job back to implementation and the fix loop terminates at the configured limit.
7. **AC-07 Isolation:** Two concurrent jobs in one project complete without interfering; the main working tree is never modified.
8. **AC-08 Security essentials:** SEC-001 – SEC-015 (Must) pass their automated tests.
9. **AC-09 Observability:** Every job has a complete event history, transcripts, context manifests, and commit lineage.
10. **AC-10 Onboarding:** Median time-to-first-job ≤ 10 minutes with ≥ 5 test users.

## 28. Release Roadmap (product view)

| Release | Theme | Contents (high level) |
|---|---|---|
| **0.1 Alpha (internal)** | Vertical slice | One provider (Claude Code), `fast` profile, worktree, gates, local merge, CLI only |
| **0.2 Alpha** | Supervised workflow | Both providers, `standard` profile, approvals, review, verification, dashboard (core screens), budgets, recovery |
| **0.3 Beta (MVP)** | Reliability & UX | `strict` profile, PR delivery, resources, FTS, token analytics, crash-injection suite green, security essentials |
| **0.4** | Intelligence | Historical estimation, scoring router, project facts, repo map, lessons, evaluation suite |
| **0.5** | Releases & polish | Release entity, notes, tags; container isolation option; light theme |
| **1.0 GA** | Commercial launch | Licensing (Team features), license files, docs, signed releases |
| **2.x** | Turtle Flow Server | Server, workers, OIDC, RBAC, Postgres, organization policies |

Engineering roadmap with dependencies: [ROADMAP.md](ROADMAP.md).
