# Dashboard UX — Structure, Screens, Journeys and Visual Design

| Field | Value |
|---|---|
| Document | DASHBOARD-UX.md |
| Status | Proposal; technology decision in [ADR-003](adr/ADR-003-embedded-webserver-dashboard.md) |
| Related | [PRD.md §12, §20](PRD.md#20-dashboard-requirements) (UX-xxx), [images/README.md](images/README.md) |

### Implementation status (2026-10-26)

The live adapter now uses these screen structures for the data available from the daemon: overview, projects/detail, jobs/detail, runs/detail, approvals/review, token analytics, and providers. Project creation, job creation/start, and budget updates are available in full sessions; the server rejects writes from read-only sessions. Resources display the designed empty state; settings and license are read-only. Currency analytics, time-series charts, provider configuration/discovery, resource ingestion, policy provenance, and license entitlements remain unavailable and are shown as unknown or unavailable rather than inferred. The mock adapter remains the visual reference and retains its illustrative data.

---

## 1. UX Principles

1. **The CLI drives, the dashboard oversees.** Every state-changing action exists in the CLI (UX-007). The dashboard excels at overview, diff review, approvals, analytics and live monitoring.
2. **Show what is bound.** Approval screens show exactly the artifact revision, hash prefix and invalidation conditions (UX-004).
3. **Measured ≠ estimated ≠ unknown.** Three distinct visual treatments everywhere (UX-006): measured = solid value; estimated = value with `≈` and range bar, dashed outline; unknown = grey `unknown` pill.
4. **Calm by default, loud when needed.** Neutral dark surfaces; color reserved for state (success, warning, danger, AI activity) and for the single primary action per view.
5. **Dense, not cramped.** 13–14 px base text, 32 px table rows, 8 px spacing grid; tables are the primary component.
6. **Live but stable.** Real-time updates never reorder rows under the cursor; new items appear with a subtle highlight and "N new" affordance.

## 2. Information Architecture

```text
┌ Sidebar (collapsible, 232 px) ───────────────┐
│  ◆ Turtle Flow          [⌘K]            │
│  Overview                                     │   1 Main Dashboard
│  Projects                                     │   2 Project Overview → 3 Project Detail
│  Jobs                                         │   (global list) → 4 Job Detail
│  Approvals            (badge: pending count)  │   6 Approvals → 7 Code Review & Diff
│  Runs                                         │   5 Agent Runs & Logs
│  Analytics                                    │   8 Token Usage & Cost
│  Providers                                    │   9 Providers & Models
│  Resources                                    │   10 Resources (also per project)
│  Settings                                     │   11 Settings & Policies
│  ─────────────                                │
│  License: Free         daemon ● connected     │   12 Licensing
└───────────────────────────────────────────────┘
Top bar: breadcrumb · project switcher · global search · live connection indicator · theme toggle
```

URL scheme: `/`, `/projects`, `/projects/:slug`, `/projects/:slug/jobs/:key`, `/jobs/:key/runs/:runId`, `/approvals`, `/approvals/:id`, `/jobs/:key/review`, `/analytics`, `/providers`, `/resources`, `/settings`, `/license`. All views deep-linkable; CLI prints dashboard URLs.

## 3. Visual Design System

### 3.0 Brand

- **Logo:** blue sea turtle with motion trails ([assets/turtle-flow-logo.png](assets/turtle-flow-logo.png), transparent PNG; original in [assets/turtle-flow-logo-original.png](assets/turtle-flow-logo-original.png)). Square icons for favicon/app use: [`turtle-flow-icon-{32,64,256,512}.png`](assets/turtle-flow-icon-256.png).
- **Brand palette (sampled from logo):** sky `#00A0F0`, brand blue `#0080F0`, deep blue `#0050F0`, navy `#001040`. UI accent uses `#2E9BFF`, a lighter tint of the brand blue that meets WCAG AA contrast on the dark background.
- **Usage:** the logo sits on dark surfaces without a container. Minimum width is 48 px; below that, use the square icon. Do not recolor the logo or place it on mid-blue backgrounds.
- **Wordmark:** logo + "Turtle Flow" in Asap Sharp 700 at 105% width, letter-spacing −1.5%. "Flow" is set in the accent blue. Variants: [on dark](assets/turtle-flow-wordmark-on-dark.png) and [on light](assets/turtle-flow-wordmark-on-light.png); the HTML sources are in [assets/src/](assets/src/wordmark-dark.html).
- **Open item:** an SVG master and a monochrome variant are needed. The favicon at 16–32 px loses the motion trails, so it should use a simplified mark.

### 3.1 Color tokens (dark-first)

| Token | Dark | Light | Use |
|---|---|---|---|
| `--bg` | `#0B0D12` | `#F7F8FA` | Page background |
| `--surface` | `#12151C` | `#FFFFFF` | Cards, tables |
| `--surface-2` | `#181C25` | `#F0F2F5` | Hover, nested panels |
| `--border` | `#252A36` | `#DDE1E8` | Dividers, outlines |
| `--text` | `#E6E8EE` | `#141821` | Primary text (contrast 15.2:1 on bg) |
| `--text-muted` | `#9AA3B2` | `#596273` | Secondary text (≥ 4.5:1) |
| `--accent` | `#2E9BFF` | `#0A6FD6` | Primary actions, focus ring, links (brand blue from logo; 6.7:1 on `--bg`, 4.9:1 on white) |
| `--ai` | `#A78BFA` | `#7048E8` | AI activity (running agents, AI findings) |
| `--success` | `#3FB950` | `#1A7F37` | Passed gates, delivered |
| `--warning` | `#D29922` | `#9A6700` | Pending approval, soft budget |
| `--danger` | `#F85149` | `#CF222E` | Failures, blockers, hard budget |
| `--info` | `#39C5CF` | `#0E7C86` | Informational states (cyan, kept distinct from the brand-blue accent) |
| `--unknown` | `#6E7681` | `#8C959F` | Unknown values |

Color is never the only carrier of meaning: every state also has an icon and label (UX-009).

### 3.2 Typography and spacing

- UI and wordmark: **Asap Sharp** (variable: `wght` 100–900, `wdth` 75–125; OFL-1.1; [assets/fonts/](assets/fonts/OFL.txt)). Scale 13/14/16/20/24 px; weights 400 (body), 500 (labels, table headers), 600 (headings), 650–700 (wordmark, KPI values). Width stays at 100% in UI; condensed widths (85–90%) are allowed only for dense table cells.
- Numbers: `font-variant-numeric: tabular-nums` (Asap Sharp ships `tnum`).
- Code, hashes, IDs, logs: **JetBrains Mono** 12.5/13 px (Asap Sharp is proportional).
- Minimum UI text size 13 px; secondary metadata 12 px only in monospace or uppercase section labels.
- Spacing: 4/8/12/16/24/32 px; radius 6 px (controls) / 10 px (cards); 1 px borders; shadows only for overlays.
- Fonts are self-hosted via `@fontsource-variable/asap-sharp` and `@fontsource-variable/jetbrains-mono` (no font CDN; CSP `self`).

### 3.2a Icons

- Set: **Fluent UI System Icons** (Microsoft, MIT) from Iconify's [`fluent`](https://icon-sets.iconify.design/fluent/) collection, compiled at build time (`unplugin-icons` + `@iconify-json/fluent`). There are no runtime fetches ([ADR-003](adr/ADR-003-embedded-webserver-dashboard.md)).
- Sizes: `20-regular` for navigation, buttons and inline UI, rendered at 16–18 px; `20-filled` for the active or selected state; `24-regular` for empty states only.
- Color: always `currentColor`. Icons never carry meaning alone; every icon has a text label or an `aria-label`.

| Purpose | Iconify name |
|---|---|
| Overview | `fluent:board-20-regular` |
| Projects | `fluent:folder-20-regular` |
| Jobs | `fluent:task-list-ltr-20-regular` |
| Approvals | `fluent:approvals-app-20-regular` |
| Runs (agents) | `fluent:bot-20-regular` |
| Analytics | `fluent:data-area-20-regular` |
| Providers | `fluent:plug-connected-20-regular` |
| Resources | `fluent:document-multiple-20-regular` |
| Settings | `fluent:settings-20-regular` |
| Search / command palette | `fluent:search-20-regular` |
| New / add | `fluent:add-20-regular` |
| Pause / resume | `fluent:pause-20-regular` / `fluent:play-20-regular` |
| Export | `fluent:arrow-download-20-regular` |
| Blocked / failure | `fluent:dismiss-circle-20-regular` |
| Warning / flag | `fluent:warning-20-regular` / `fluent:flag-20-regular` |
| Diff / code | `fluent:code-20-regular` |
| Branch | `fluent:branch-20-regular` |
| Security / policy | `fluent:shield-checkmark-20-regular` |

### 3.3 Core components

| Component | Notes |
|---|---|
| **Stage stepper** | Horizontal chain of stages; current stage pulsing ring; loops shown as small `↺2` counters; approval stages diamond-shaped |
| **State badge** | Pill with icon + label per job state; holds shown as secondary pill (`⏸ paused`, `⏳ rate-limited`) |
| **Estimate bar** | Range P10–P90 with P50 tick; measured actual overlaid as solid marker; confidence label |
| **Budget meter** | Stacked bar: settled (solid), reserved (hatched), headroom; limit line; hard/soft indicator |
| **Hash chip** | `sha256:91af…3c` monospaced, copy button, tooltip with full hash |
| **Diff viewer** | Unified/split toggle, file tree, inline AI findings (severity colored gutter), "viewed" checkboxes |
| **Log stream** | Virtualized, ANSI-stripped, tool calls collapsible, autoscroll lock, `aria-live="polite"` throttled |
| **Data table** | Sortable, column chooser, sticky header, keyboard row navigation, CSV export |
| **Command palette** | `⌘K`: navigation, "approve…", "pause job…", search |
| **Toasts/banners** | Connection lost, budget thresholds, security events (persistent until acknowledged) |

## 4. Screens

Permission notes: in the MVP there is one local user with all permissions; the "Permissions" rows describe Phase 2 roles (`viewer`, `developer`, `approver`, `project_admin`, `org_admin`, `auditor`).

### 4.1 Main Dashboard

Route: `/` · Requirement: UX-020 · Mockup: [dashboard-overview.png](images/dashboard-overview.png)

| Aspect | Specification |
|---|---|
| Purpose | Answer "what needs me, what is running, are we within budget?" in 5 seconds |
| Layout | KPI strip (4 tiles) → two-column: left "Needs your attention" (approvals, blocked jobs) + "Active jobs" table; right "Live agent activity", "Budget (this month)", "Provider health" |
| Components | KPI tiles (active jobs, pending approvals, tokens this month vs. budget, verified jobs 30 d); approval cards; active jobs table (key, title, project, stage stepper mini, provider, tokens, elapsed); live activity feed; budget meters; provider status list |
| User actions | Open approval; quick approve from card only for `scope` with diff-free subject (opens confirm sheet showing hash); pause/resume job; jump to job |
| Data | `/api/v1/overview` aggregate + SSE `job.*`, `approval.*`, `run.*`, `budget.*` |
| Real-time | Stage changes, new approvals (badge + toast), token counters (measured increments), provider health changes |
| Empty state | "No projects yet" with `turtle project add <path>` command and copy button; "No active jobs" with create-job CTA |
| Error state | Daemon unreachable: full-width banner "Disconnected — retrying (3)"; last data shown dimmed with timestamp |
| Permissions | viewer: read; approver: approve actions |

### 4.2 Project Overview

Route: `/projects` · Requirement: UX-021 · Mockup: [dashboard-projects.png](images/dashboard-projects.png)

| Aspect | Specification |
|---|---|
| Purpose | Compare projects; navigate |
| Layout | Toolbar (search, filter status, sort, "Add project") → table (default) or card grid |
| Components | Project row: name + slug, repo path, stack icons, active jobs, pending approvals, month tokens vs. budget meter, gate pass rate (30 d), last delivery, health dot |
| User actions | Add project (shows CLI equivalent + path picker in Phase 2), archive, open |
| Data | `/api/v1/projects?include=stats` |
| Real-time | Counters update via SSE |
| Empty state | Onboarding panel: 3 steps (install provider CLI, `turtle project add`, create job) |
| Error state | Project repo missing → row warning "Repository not found at path" + "Relink" action |
| Permissions | viewer: list granted projects; project_admin: archive; org_admin: add |

### 4.3 Project Detail

Route: `/projects/:slug` · Requirement: UX-022 · Mockup: prompt in [images/README.md](images/README.md#image-generation-prompts-for-the-remaining-six-screens)

| Aspect | Specification |
|---|---|
| Purpose | Everything about one project |
| Layout | Header (name, repo, default branch, profile, health) → tabs: **Jobs** · Resources · Knowledge · Budgets · Settings · History |
| Components | Jobs table with state filters; budget panel; gate pass-rate sparkline; provider mix donut; commands list (build/test/lint) with "last result"; knowledge items list (stale highlighted) |
| User actions | Create job (form: title, description, type, profile, priority, attachments, budget override, dry-run toggle); edit settings; attach resource; run `doctor` |
| Data | Project, jobs, budgets, knowledge, resources |
| Real-time | Job rows, budgets |
| Empty state | "No jobs yet — create your first job" with example job text |
| Error state | Settings invalid (e.g. test command missing) → inline validation and doctor results |
| Permissions | developer: create jobs; project_admin: settings |

### 4.4 Job Detail and Workflow Timeline

Route: `/projects/:slug/jobs/:key` · Requirement: UX-023 · Mockup: [dashboard-job-detail.png](images/dashboard-job-detail.png)

| Aspect | Specification |
|---|---|
| Purpose | Single source of truth for a job's progress and evidence |
| Layout | Header (key, title, state badge, holds, profile, primary action) → stage stepper (full) → three columns: left **Timeline** (events grouped by stage, loops visible), center **Tasks** (DAG list with status, commits, gates) and **Artifacts** (spec, plan, review, verification — revision selector), right **Tokens** (estimate bar vs. actual, per-role breakdown, budget meter) and **Approvals** (status + bound hashes) |
| Components | Stepper, timeline, task list with expandable gate results, artifact viewer (rendered Markdown of structured artifacts + JSON toggle), estimate bar, approval chips |
| User actions | Pause (soft/hard), resume, cancel (confirm with key), resolve block (choose target), edit spec/plan (creates revision), override provider/model for next run, open review, export report |
| Data | `/api/v1/jobs/:id?include=tasks,artifacts,approvals,usage,events` |
| Real-time | Stage transitions, task state, gate results, token increments, new revisions |
| Empty state | Pre-start job: shows preflight report and "Start" |
| Error state | Blocked: red panel with reason, error code, remediation, allowed targets as buttons |
| Permissions | viewer read; developer: pause/resume/edit; approver: approvals |

### 4.5 Agent Runs and Execution Logs

Route: `/runs`, `/jobs/:key/runs/:id` · Requirement: UX-024 · Mockup: prompt in [images/README.md](images/README.md#image-generation-prompts-for-the-remaining-six-screens)

| Aspect | Specification |
|---|---|
| Purpose | Inspect what an agent did, with what context, at what cost |
| Layout | List view: filterable table (run, job, role, provider/model, status, duration, tokens w/ provenance, cost). Detail: header (role, provider, model, CLI version, routing reason) → tabs **Transcript** · Context manifest · Usage · Changed files · Raw |
| Components | Log stream with tool-call blocks; context manifest table (item, source, trust, inclusion reason, ≈tokens); usage table (input/cached/output/reasoning with provenance badges); routing decision card (candidates + scores) |
| User actions | Cancel running run; copy invocation (redacted); download transcript; "rerun with different model" (creates override + retry command) |
| Data | Run, transcript pages (`?cursor=`), manifest artifact, usage record |
| Real-time | Transcript streaming via SSE (`run.output` events, throttled) |
| Empty state | "No runs yet" |
| Error state | Transcript blob expired by retention → "Raw transcript pruned (retention 90 d); summary available" |
| Permissions | viewer read (transcripts may be restricted to developer+ in Phase 2) |

### 4.6 Human Approvals

Route: `/approvals`, `/approvals/:id` · Requirement: UX-025 · Mockup: [dashboard-approvals.png](images/dashboard-approvals.png)

| Aspect | Specification |
|---|---|
| Purpose | Make correct approval decisions quickly, with full knowledge of what is bound |
| Layout | Left: queue (grouped by gate kind; age; job; risk flags). Right: approval detail: header (gate kind, job, requested at, expiry) → **What you are approving** box (artifact list with revision + hash chips, base/head commits, budget envelope, profile) → content (spec rendered with criteria table; or diff summary; or budget delta) → **Flags** (protected paths, dependency changes, scope conformance, low-confidence estimate) → **Invalidated if** list → decision bar (Approve · Request changes · Reject; comment box) |
| Components | Queue list, binding box, criteria table, estimate bar, flag list with required acknowledgement checkboxes, sticky decision bar with keyboard shortcuts (`A`, `C`, `R` with confirmation) |
| User actions | Approve (sends `subject_hash`), request changes (comment required), reject (comment required), acknowledge flags, open full diff |
| Data | Approval + binding + referenced artifacts |
| Real-time | If the subject changes while open → banner "This approval was invalidated (spec revised to rev 4). Reload to review the new revision." and decision bar disabled |
| Empty state | "No pending approvals. Jobs will appear here when they need your decision." |
| Error state | Hash mismatch on submit → error "The approval subject changed. Nothing was approved." |
| Permissions | approver only for decisions; policy may forbid approving own jobs (Phase 2) |

### 4.7 AI Code Review and Git Diff

Route: `/jobs/:key/review` · Requirement: UX-026 · Mockup: [dashboard-code-review.png](images/dashboard-code-review.png)

| Aspect | Specification |
|---|---|
| Purpose | Review the job diff with AI findings in context |
| Layout | Top: summary bar (files, +/−, findings by severity, gates status, reviewer provider/model) → left file tree (with finding counts, protected-path badges) → main diff (split/unified) with inline finding cards → right panel: findings list (filter by severity/category/status) |
| Components | Diff viewer (virtualized, syntax highlighting), finding card (severity, category, rationale, suggested fix, status: open/fixed/dismissed/acknowledged), gate summary chips |
| User actions | Mark file viewed; dismiss finding with reason; request fix for selected findings (creates fixer task → job goes back to implementation); proceed to acceptance approval |
| Data | Diff (paged per file), review report, gate results |
| Real-time | Fix iterations update diff and findings (banner "New revision available") |
| Empty state | "No changes" (diff empty) — blocks acceptance |
| Error state | Diff too large to render → per-file lazy loading; binary files summarized |
| Permissions | developer: dismiss/request fix; approver: proceed |

### 4.8 Token Usage and Cost Analytics

Route: `/analytics` · Requirement: UX-027 · Mockup: [dashboard-token-analytics.png](images/dashboard-token-analytics.png)

| Aspect | Specification |
|---|---|
| Purpose | Understand where tokens go, estimate accuracy, cost per verified job |
| Layout | Filters (period, project, provider, model, role, billing mode) → KPI strip (total tokens, API-equivalent/actual cost, cost per verified job, estimate coverage) → charts: usage over time (stacked by provider/model), breakdown by role/stage, estimate vs. actual scatter with P10–P90 bands, cache-read ratio → tables: top jobs by tokens, provider/model efficiency |
| Components | Stacked area chart, horizontal bars, scatter, tables; provenance legend (measured / estimated) |
| User actions | Change grouping; export CSV; open job; edit price table (link) |
| Data | `/api/v1/analytics/*` |
| Real-time | Optional "live" toggle (refresh every 30 s) |
| Empty state | "Analytics appear after the first completed run" |
| Error state | Price table missing for a model → costs shown as "unknown" with "Add price" link |
| Permissions | viewer: own projects; org_admin: cross-project (Phase 2) |

### 4.9 Provider and Model Management

Route: `/providers` · Requirement: UX-028 · Mockup: prompt in [images/README.md](images/README.md#image-generation-prompts-for-the-remaining-six-screens)

| Aspect | Specification |
|---|---|
| Purpose | See what providers can do, their health, and configure routing preferences and prices |
| Layout | Provider cards (binary, version, compat status, auth, last discovery) → capability matrix table (rows: capabilities; columns: providers; cells Known/Unsupported/Unknown) → models table (tier, context window, price, enabled) → routing preferences (role × tier mapping) → rate-limit history |
| User actions | Refresh discovery; run contract self-test; enable/disable model; edit tier mapping; edit price table (new version) |
| Data | Capabilities, price tables, limits observed |
| Real-time | Health changes, rate-limit events |
| Empty state | "No provider CLIs found" with install/auth instructions per provider |
| Error state | `Untested` CLI version → amber banner with "Run self-test" |
| Permissions | org_admin / project_admin |

### 4.10 Resource Management

Route: `/resources` · Requirement: UX-029 · Mockup: prompt in [images/README.md](images/README.md#image-generation-prompts-for-the-remaining-six-screens)

| Aspect | Specification |
|---|---|
| Purpose | Manage reference material and control what agents can see |
| Layout | Table (title, kind, trust, versions, attached to, size, extraction status) → detail drawer: versions list with hashes, extraction preview (chunks with headings), attachment scopes, "used in N context packages" |
| User actions | Upload/add (file, URL snapshot), new version, change trust level, attach/detach to project/job/task, pin, delete (if unused or after confirmation) |
| Data | Resources, versions, chunks, usage in manifests |
| Real-time | Extraction progress |
| Empty state | Explains that resources are only sent when relevant |
| Error state | Extraction failed (e.g. scanned PDF) → "No text extracted; OCR not supported in this version" |
| Permissions | developer: add/attach; project_admin: trust changes to `trusted` |

### 4.11 Settings and Policies

Route: `/settings` · Requirement: UX-030 · Mockup: prompt in [images/README.md](images/README.md#image-generation-prompts-for-the-remaining-six-screens)

| Aspect | Specification |
|---|---|
| Purpose | Configure profiles, gates, protected branches/paths, security policy, budgets, retention |
| Layout | Left nav: General · Workflow profiles · Gates · Git & delivery · Security · Budgets · Retention · Notifications · About → right: form sections with "effective value" + "source" (default / Turtle Flow / repo / override) |
| Components | Profile editor (diff vs. base profile), protected path list, toggle with policy-exception warning, source badges |
| User actions | Edit and save (creates new policy version; shows which active jobs are affected and which approvals would be invalidated) |
| Data | Resolved config + sources |
| Empty state | n/a |
| Error state | Validation errors inline; attempt to relax via repo file shown as "ignored (tighten-only)" |
| Permissions | project_admin / org_admin; org-enforced policies locked with lock icon (Business+) |

### 4.12 Licensing and Subscription

Route: `/license` · Requirement: UX-031 · Mockup: prompt in [images/README.md](images/README.md#image-generation-prompts-for-the-remaining-six-screens)

| Aspect | Specification |
|---|---|
| Purpose | Show tier and entitlements; import license; clarify AI provider costs are separate |
| Layout | Current tier card → entitlements table → license details (id, licensee, expiry, seats) → import area → info box "AI provider costs are billed by your providers and are not part of your Turtle Flow license" |
| User actions | Import license file; remove license; copy installation ID |
| Empty state | Free tier: "You're on Free. Everything for a single developer is included." + compare tiers link |
| Error state | Invalid signature / expired: clear explanation, grace period countdown, assurance that data stays accessible |
| Permissions | org_admin |

## 5. Key User Journeys

### J-A: First job (onboarding, target ≤ 10 min)

1. `turtle init` → detects Git, Claude Code/Codex, auth status → offers `turtle project add .`
2. Proposes build/test/lint commands → user confirms → validation run.
3. `turtle job create "…" --start` → CLI prints dashboard URL.
4. Spec appears → Approvals screen → user reviews criteria and estimate → approves.
5. Job detail shows live implementation; user leaves.
6. Desktop notification: acceptance needed → Code Review → Approve → delivered.

### J-B: Budget pause

Run-level reservation denied → job hold `awaiting_budget` → banner on overview + approval of type `budget_increase` showing actual vs. envelope and remaining work estimate → user raises job budget by 300k → run starts.

### J-C: Blocked by failing tests

Fix limit reached → Job detail red panel: failing test names, last fixer summary, options (give 2 more fix iterations, replan, open worktree path for manual fix, cancel) → user picks manual: `turtle job worktree API-42` prints path → user fixes and commits → `turtle job resume API-42` → gates rerun (user commits recorded as `author_kind=user`).

### J-D: Approval invalidated while reviewing

User viewing acceptance; base branch moves; Turtle Flow refreshes and reruns gates → approval invalidated → live banner disables decision bar → new approval appears with new diff hash.

## 6. Accessibility and Responsiveness

- WCAG 2.2 AA: contrast tokens verified; focus ring 2 px `--accent`; all actions keyboard-reachable; skip links; ARIA live regions throttled for streams; reduced-motion support (no pulsing).
- Breakpoints: ≥ 1440 (three columns), 1280 (default), 1024 (right column collapses to tabs), 768 (sidebar overlay), ≥ 375 (single-column job and approval evidence). The mobile preview remains read-only below 550 px. A `turtle dashboard --phone` session is read-only at every viewport, enforced by the API session scope; its live screen hides all decision and command controls. Approval hashes and bound documents must wrap within a 390 px viewport.

## 7. Dashboard Mockup Images

Six standalone 16:9 (1920×1080) mockups were rendered from HTML/CSS sources with headless Chrome (not AI-generated). Files and image-generation prompts for the remaining six screens: [images/README.md](images/README.md).

| Screen | Image |
|---|---|
| Main Dashboard | [images/dashboard-overview.png](images/dashboard-overview.png) |
| Project Overview | [images/dashboard-projects.png](images/dashboard-projects.png) |
| Job Detail & Timeline | [images/dashboard-job-detail.png](images/dashboard-job-detail.png) |
| Approvals | [images/dashboard-approvals.png](images/dashboard-approvals.png) |
| Code Review & Diff | [images/dashboard-code-review.png](images/dashboard-code-review.png) |
| Token Analytics | [images/dashboard-token-analytics.png](images/dashboard-token-analytics.png) |
