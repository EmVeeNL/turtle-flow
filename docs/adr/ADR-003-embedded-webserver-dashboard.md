# ADR-003: Embedded Webserver and Dashboard

- **Status:** Accepted (revised 2026-10-24: framework comparison with Astro and TanStack Start; typography and icon set). **Amended 2026-10-25: the dashboard uses Astro + Tailwind CSS v4 (product owner decision); see *Amendment* below. Where this ADR says Vite + React + TanStack, the amendment wins.**
- **Date:** 2026-10-24

## Context

Developers need a visual overview, diff review, approvals and analytics (PRD §20) in addition to the CLI. The product is local-first and ships as one binary. Live updates (agent output, stage changes) are essential (UX-003). The Rust daemon is the backend: it owns the API, auth and all state. Brand decisions: typeface **Asap Sharp** for logo and UI; icon set **Fluent UI System Icons** (via Iconify).

Candidate frontend stacks proposed by the product owner: **Astro**, **Vite + React + TanStack Router**, or **TanStack Start**.

## Problem Statement

How should the dashboard be built, served, and kept live — and which frontend framework fits an app embedded in a Rust binary — without adding runtime dependencies or weakening local security (no CDN, strict CSP, offline)?

## Decision

1. The daemon embeds an **Axum** HTTP server bound to loopback ([ADR-012](ADR-012-agent-execution-security.md), SEC-001) serving a versioned JSON API (`/api/v1`) and the dashboard.
2. The dashboard is a **client-side single-page application: Vite + React + TypeScript + TanStack Router** (file-based routes, automatic code splitting, typed search params), with **TanStack Query** for server state, **TanStack Table** + **TanStack Virtual** for large tables and log streams, Tailwind CSS driven by the design tokens in [DASHBOARD-UX.md §3](../DASHBOARD-UX.md#3-visual-design-system), and Radix UI primitives for accessible overlays/menus. Build output (`dist/`) is embedded with `rust-embed` (precompressed, hashed filenames, immutable caching); Axum serves `index.html` for unknown non-API paths (SPA fallback).
3. **Not TanStack Start, not Astro** for the dashboard (see alternatives). **Astro (with Starlight) is the recommended choice for the future public docs/marketing site**, which is content-first and statically hosted — a different problem.
4. **Server-Sent Events** (`/api/v1/events/stream`) deliver live updates; clients resume via `Last-Event-ID` = event `seq`; SSE events invalidate/patch TanStack Query caches. Commands use normal HTTP requests.
5. The **CLI and dashboard use the same API** (CLI via Unix socket). OpenAPI is generated with `utoipa`; the TypeScript client and types are generated from it (e.g. `openapi-typescript` + `openapi-fetch`).
6. **Typography:** **Asap Sharp** (variable; `wght` 100–900, `wdth` 75–125; OFL-1.1) for UI and the wordmark, self-hosted via `@fontsource-variable/asap-sharp` (latin + latin-ext subsets, ~100 KB woff2). It ships `tnum`, so numeric columns use `font-variant-numeric: tabular-nums`. Asap Sharp is proportional, so code, hashes, IDs and logs use **JetBrains Mono** (OFL, `@fontsource-variable/jetbrains-mono`).
7. **Icons:** **Fluent UI System Icons** (MIT) from Iconify's `fluent` set, compiled at **build time** with `unplugin-icons` + `@iconify-json/fluent` into React components — only icons actually imported end up in the bundle; **no runtime Iconify API calls** (the CSP `connect-src 'self'` and offline requirement forbid them). Convention: `20-regular` for UI chrome, `20-filled` for selected/active states, `24-*` only for empty states.

## Considered Alternatives

| Alternative | Assessment |
|---|---|
| **TanStack Start** | Full-stack React framework (SSR, streaming, server functions, server routes) built on TanStack Router + Vite. Its core value is a *JS server runtime*; ours is Rust/Axum, so server functions, SSR and its deployment adapters would go unused or require a Node process next to the daemon — contradicting the single-binary decision ([ADR-001](ADR-001-local-first-modular-monolith.md)). SPA/static modes exist, but then it reduces to TanStack Router with extra build machinery and a faster-moving API surface. Choosing plain TanStack Router keeps a migration path open: routes and loaders port to Start if a JS server ever becomes useful (e.g. a hosted cloud edition). |
| **Astro** | Excellent for content sites: islands, zero-JS-by-default, static output that would embed fine. The dashboard is the opposite workload: an authenticated, highly interactive app with shared client state, a persistent SSE connection, live tables, diff viewer and command palette across all views. In Astro this becomes one large React island (Astro adds nothing) or many islands that must share state and an SSE connection across page navigations (friction). Recommended instead for the public docs/marketing site. |
| SvelteKit (adapter-static) | Smaller bundles, pleasant DX. Viable; React chosen for its ecosystem of mature components (diff viewers, virtualized tables, charts) and broad familiarity among contributors and AI coding agents. |
| Leptos / Dioxus (Rust → WASM) | Single language, but immature component ecosystem for diff/chart UIs; slower iteration; larger WASM bundles |
| Server-rendered HTML (Askama + htmx) | Very light; weaker for rich diff review and live charts; viable for a "lite" mode |
| React Router (framework mode) / Next.js | Next.js targets a Node/edge server runtime (same mismatch as Start); React Router is fine but TanStack Router gives stronger type-safety for search params (filters, pagination) used heavily in analytics and job lists |
| Iconify runtime component (`@iconify/react` fetching from API) | Fetches icon data from `api.iconify.design` at runtime — breaks offline use and strict CSP; only acceptable with `addCollection` of a bundled subset, which `unplugin-icons` does more efficiently |
| Fluent icons via `@fluentui/react-icons` | Official package, but large and tied to Fluent UI React conventions; Iconify JSON + build-time compilation is lighter and matches the chosen source |
| Google Fonts CDN for Asap Sharp | Network dependency, privacy leak, violates CSP `default-src 'self'`; self-hosting is required |
| WebSockets | Bidirectional not needed; SSE is simpler, HTTP-native, auto-reconnects, works with cookie auth |
| Separate frontend server | Breaks single-binary goal |

## Rationale

The dashboard is a rich, private, local client; an SPA over the Rust JSON API is the simplest correct model. TanStack Router delivers what a framework would add that matters here (typed routing, loaders, code splitting) without a JS server. Embedding keeps distribution trivial. Self-hosted fonts and build-time icons keep the app fully offline and CSP-strict.

## Consequences

- Frontend build is part of the release pipeline (Node toolchain in CI only, never at runtime).
- API must be stable and versioned; CLI JSON output and dashboard share generated types.
- Bundle adds a few MB to the binary; fonts ~150 KB (UI + mono), icons only as used.
- No SSR: first paint shows an app shell; acceptable for a localhost tool (NFR-003 target ≤ 2 s).
- Proposed initial versions (at time of writing, pin in lockfile): Vite 8.x, React 19.x, `@tanstack/react-router` 1.x, `@fontsource-variable/asap-sharp` 5.x, `@iconify-json/fluent` 1.2.x, `unplugin-icons` 24.x.

## Risks

- Frontend dependency supply-chain risk.
- SSE connections limited per browser host (HTTP/1.1 six-connection limit).
- Asap Sharp is relatively new; rendering/hinting at small sizes (11–12 px) on non-retina Linux displays may be weaker than Inter.

## Mitigations

- Lockfile, `--frozen-lockfile`, audit in CI; no runtime CDN usage; strict CSP.
- One multiplexed SSE stream per tab with server-side filtering by subscription parameters.
- Minimum UI text size 13 px; visual regression tests (TST-18) on Linux and macOS; fallback stack `"Asap Sharp", system-ui, sans-serif`.

## Related ADRs

[ADR-001](ADR-001-local-first-modular-monolith.md), [ADR-012](ADR-012-agent-execution-security.md), [ADR-016](ADR-016-observability-auditability.md)

## Amendment (2026-10-25): Astro and Tailwind CSS v4 instead of Vite + React + TanStack

**Decision.** The product owner chose **Astro** with **Tailwind CSS v4** for the dashboard. The earlier comparison rated Astro as a poor fit for an interactive app; that concern is addressed by how the dashboard is built, not ignored:

- **Static output only** (`output: 'static'`), embedded with `rust-embed` as before. No Node runtime, no SSR, no Astro server.
- **Islands for interactivity.** Most pages are static Astro components; the command palette, approval decision, diff viewer, virtualized log stream and detail lookup are **Preact** islands (small, React-like, enough for shared local state and keyboard handling). Islands hydrate only on pages that use them.
- **Routing for IDs unknown at build time.** Detail views are static pages that read the ID from the query string (`/jobs/view?id=API-42`), so no `getStaticPaths` and no per-entity HTML. The Rust server serves plain files; it needs no SPA fallback for these routes.
- **Tailwind CSS v4** via `@tailwindcss/vite`, with the tokens from DASHBOARD-UX §3 declared in `@theme`.
- **Icons** via `astro-icon` + `@iconify-json/fluent`, resolved at build time (replaces `unplugin-icons`; same no-runtime-fetch property). Fonts unchanged: self-hosted Asap Sharp and JetBrains Mono.
- **CSP:** Astro's built-in CSP support emits hashes for its own scripts and styles; no `unsafe-inline`. Interactive behavior that is not an island (theme, copy, sort, filter) ships as same-origin external scripts.

**What this gives up.** TanStack Router's typed search params, TanStack Query's cache, and the React component ecosystem (diff viewers, charts) are not available off the shelf; the API client, SSE reconnect logic and the virtualized log are written in `dashboard/src/lib`. Client-side navigation between pages is full page loads, which is acceptable on loopback. If interactive state outgrows islands (for example many cross-linked live panels), revisit this decision before building more of the app inside islands: the data layer is framework-neutral TypeScript, so a move to an SPA would reuse it.

**Unchanged in the design:** Axum on loopback, `/api/v1`, SSE with `Last-Event-ID`, eventual CLI and dashboard API reuse, and the typography and icon-set decisions. Astro with Starlight remains the recommendation for the future public docs site.

The implementation and its deviations from DASHBOARD-UX.md are documented in [`dashboard/README.md`](../../dashboard/README.md).

## Implementation note (M4)

The daemon now serves Axum and the static Astro build on loopback. `rust-embed` is an opt-in Cargo feature; source builds can serve `dashboard/dist` or `TURTLE_DASHBOARD_DIR` and do not require a frontend build. Built HTML's Astro CSP hashes are copied to the HTTP header. SSE resumes from a database event sequence using a bounded cursor query and fallback poll. The real dashboard build uses a client view that reads the local API, while the richer static layouts remain the mock design preview. The CLI still opens SQLite directly, and the Unix-socket transport and OpenAPI generation from the original decision remain unimplemented.
