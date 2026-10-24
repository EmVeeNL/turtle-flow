# Dashboard Mockup Images

## How these images were produced

The six PNG files in this directory are **real rendered images**, but they are **not AI-generated**. Each screen was hand-authored as static HTML/CSS (sources in [`src/`](src/); font Asap Sharp from [`../assets/fonts/`](../assets/fonts/OFL.txt), icons from the Fluent set via [`src/fluent-icons.json`](src/fluent-icons.json)) with the Turtle Flow logo from [`../assets/`](../assets/) and the design tokens from [DASHBOARD-UX.md §3](../DASHBOARD-UX.md#3-visual-design-system), then rendered at 1920×1080 (16:9) with headless Google Chrome. Each image is a single standalone screen — no collages or contact sheets.

Regenerate:

```bash
python3 src/generate.py src                       # writes src/dashboard-*.html
for f in src/dashboard-*.html; do
  "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" --headless=new --hide-scrollbars \
    --window-size=1920,1080 --screenshot="$(basename "$f" .html).png" "file://$PWD/$f"
done
```

All screens share one sidebar, top bar, typography and component style ([`src/shared.css`](src/shared.css)), and one consistent fictional dataset (project `payments-api`, job `API-42`, 21.3M tokens this month, etc.). All data shown is illustrative. Model identifiers are deliberately shown as placeholders (`<standard-model>`), consistent with the rule that Turtle Flow does not hardcode vendor/model assumptions.

| File | Screen | Spec |
|---|---|---|
| [dashboard-overview.png](dashboard-overview.png) | Main Dashboard | [DASHBOARD-UX.md §4.1](../DASHBOARD-UX.md#41-main-dashboard) |
| [dashboard-projects.png](dashboard-projects.png) | Project Overview | [§4.2](../DASHBOARD-UX.md#42-project-overview) |
| [dashboard-job-detail.png](dashboard-job-detail.png) | Job Detail & Workflow Timeline | [§4.4](../DASHBOARD-UX.md#44-job-detail-and-workflow-timeline) |
| [dashboard-approvals.png](dashboard-approvals.png) | Human Approvals | [§4.6](../DASHBOARD-UX.md#46-human-approvals) |
| [dashboard-code-review.png](dashboard-code-review.png) | AI Code Review & Git Diff | [§4.7](../DASHBOARD-UX.md#47-ai-code-review-and-git-diff) |
| [dashboard-token-analytics.png](dashboard-token-analytics.png) | Token Usage & Cost Analytics | [§4.8](../DASHBOARD-UX.md#48-token-usage-and-cost-analytics) |

## Image-generation prompts for the remaining six screens

No images exist yet for the following screens. These prompts are written so that each produces one standalone 16:9 desktop screenshot consistent with the six rendered mockups. They can be used with an image-generation model or as a brief for a designer.

**Shared style preamble (prepend to every prompt):**

> Photorealistic UI screenshot of a desktop web application called "Turtle Flow", 1920×1080, 16:9, single screen only (no collage, no device frame, no perspective). Dark-first developer-tool aesthetic: page background #0B0D12, card surfaces #12151C with 1px borders #252A36 and 10px radius, primary text #E6E8EE, muted text #9AA3B2, accent #2E9BFF (brand blue) for primary buttons and links, violet #A78BFA for AI activity, green #3FB950 success, amber #D29922 warning, red #F85149 danger. Asap Sharp font (sharp-cornered humanist sans) for all UI text at 13–14px, JetBrains Mono for hashes, IDs and logs. Fluent UI System Icons (Microsoft, 20px regular outline style) next to every navigation item and on primary buttons. Left sidebar 232px with the Turtle Flow logo (blue sea turtle with motion trails, docs/assets/turtle-flow-logo.png) next to the product name, items Overview, Projects, Jobs, Approvals (amber badge "3"), Runs, Analytics, Providers, Resources, Settings; footer "daemon connected · v0.3.0" and "License: Free · local". Top bar 52px with breadcrumb, search field "Search jobs, runs, resources…", green "Live" indicator. Dense but readable tables with 38px rows, pill badges, no stock photos, no emoji, crisp text.

### 1. `dashboard-project-detail.png` — Project Detail

> [preamble] Breadcrumb "Projects / payments-api". Header: "Payments API" title, mono path "~/dev/payments-api", pills "default branch main · protected", "profile standard", green "healthy". Tabs row: Jobs (active), Resources, Knowledge, Budgets, Settings, History. Main left (2/3 width): jobs table with columns Job (mono blue keys API-42, API-44, API-45, API-39, API-38), Title, State pill (acceptance approval amber, implementation violet, queued grey, done green), Profile, Tokens, Updated. Right column: card "Budget · October" with stacked bar 6.4M settled + 0.4M reserved of 25M hard; card "Commands" listing build/test/lint/format_check with mono commands and green "last run ✓" ticks; card "Gate pass rate 30d" with small sparkline at 96%; card "Knowledge" listing 4 items with one amber "stale" badge. Primary button "+ New job".

### 2. `dashboard-agent-runs.png` — Agent Runs & Execution Logs

> [preamble] Breadcrumb "Jobs / API-44 / Runs / 01JBA2K7…". Header: role pill "implementer" (violet), provider "codex", model placeholder "<standard-model>", CLI version mono, status pill "running" with pulsing violet dot, elapsed "11m 42s", button "Cancel run". A card "Routing decision" showing 3 candidates with scores and the reason "tier standard; cheapest healthy candidate; reviewer must differ". Tabs: Transcript (active), Context manifest, Usage, Changed files, Raw. Transcript panel: dark log stream in JetBrains Mono, assistant messages in light text, collapsible tool-call blocks ("shell · cargo test -p api refunds" with exit code badges), a streaming cursor at the bottom, toggle "Auto-scroll". Right sidebar: usage table input/cached/output with grey "reported" provenance badges, a budget reservation bar "212k of 260k reserved", and a context manifest mini-list with trust badges (trusted/internal/untrusted).

### 3. `dashboard-providers.png` — Provider & Model Management

> [preamble] Breadcrumb "Providers". Two provider cards at top: "claude-code" (binary path mono, version, compat "Supported", auth "logged in", green healthy) and "codex" (compat "Supported", amber "rate-limited · until 09:31 (hint)"). Below, a capability matrix table: rows streaming JSON, native JSON schema, usage reporting, cost reporting, session resume, max-turns, tool allowlist, sandbox modes, image input, quota visibility; columns per provider; cells show green "Known ✓", grey "Unsupported", and dashed-outline "Unknown" pills — quota visibility is "Unknown" for both. Right: "Routing tiers" editor table (role × complexity XS–XL mapped to economy/standard/premium chips) and "Price table v7" with a note "verify with your provider; prices change". Bottom: rate-limit history timeline with small amber markers.

### 4. `dashboard-resources.png` — Resource Management

> [preamble] Breadcrumb "Resources · payments-api". Toolbar with "Add resource", filter by kind and trust. Table: Title, Kind (markdown, openapi, pdf, fixture), Trust pill (trusted green outline, internal blue, untrusted amber), Versions, Attached to (project/job chips like "API-42"), Size, Extraction status. Right drawer open for "openapi.yaml": version list with mono SHA-256 hash chips (v3 current, v2, v1), extraction preview showing chunk list with heading paths like "paths./v1/refunds.post", a note "Only relevant chunks are sent to agents (FTS ranked, within context budget)", and "Used in 14 context packages". Bottom of drawer: "Change trust level" select and "Pin to job" button.

### 5. `dashboard-settings.png` — Settings & Policies

> [preamble] Breadcrumb "Settings / Workflow profiles". Left settings navigation: General, Workflow profiles (active), Gates, Git & delivery, Security, Budgets, Retention, Notifications, About. Main: profile editor for "standard-api (base: standard)" shown as form sections — Stages, Approvals, Gates, Review, Limits — each field showing its effective value and a small source badge ("default", "global", "repo", "override"). A highlighted field "review.cross_provider = required" with badge "repo (tighten-only ✓)". A locked toggle "acceptance approval for protected branches" with lock icon and tooltip "cannot be disabled". Bottom banner: "Saving creates policy version 5 · affects 2 active jobs · 1 approval will be invalidated", buttons "Cancel" and "Save policy".

### 6. `dashboard-licensing.png` — Licensing & Subscription

> [preamble] Breadcrumb "License". Large card "You're on Free" with text "Everything for a single developer is included" and a muted "Compare tiers" link. Entitlements table with green checks for Supervised workflow, All providers, Token intelligence, Local dashboard & API, Sandboxing & audit; grey dashes for Multi-user, SSO, Server deployment, Org policies, Audit export. Card "Import license file" with dashed drop zone and mono "Installation ID 01JB…7QX" with copy button. Info box with accent border: "AI provider costs are billed by your providers and are not part of your Turtle Flow license." Small tier comparison strip: Free / Team / Business / Enterprise with "price hypotheses" footnote.
