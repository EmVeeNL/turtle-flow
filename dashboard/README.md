# Turtle Flow dashboard

The default build is a design preview backed by mock data. The `PUBLIC_API_ADAPTER=real` build loads live projects, jobs, approvals, runs, usage and events from the local daemon. Its client view shows unavailable values as unknown; the richer static layouts remain available in the mock preview.

## Run and verify

```sh
cd dashboard
npm ci
npm run dev
npm run check
npm run lint
npm test
npm run build
npx playwright install chromium
npm run test:e2e
```

The static bundle is in `dist/`. For live use, run `PUBLIC_API_ADAPTER=real npm run build`, start `turtle up --agent demo` (or a real provider), then run `turtle dashboard` in an interactive terminal. The daemon serves the bundle on its loopback port; `/login#code=…` exchanges the one-use code for a session cookie. The implemented endpoint contract is in [API-ASSUMPTIONS.md](API-ASSUMPTIONS.md).

## Structure and decisions

- `src/pages/` contains the twelve screens plus list and query-string detail routes.
- `src/layouts/AppShell.astro` contains the shared sidebar, project switcher, top bar, and theme control.
- `src/components/*.astro` renders static tables, badges, budgets, estimates, hashes, and steppers.
- Preact islands handle the command palette, approval decision, diff, live log virtualization, and query-string detail lookup. Preact was chosen because these controls share small local states and keyboard events; most dashboard HTML remains static Astro output. Islands hydrate only on pages that use them.
- `src/lib/api/` defines typed domain records and an adapter interface; `mock.ts` is the default and `real.ts` calls the daemon's same-origin API. The reconnecting SSE client is separate from its mock and browser transports.
- `public/theme.js` runs before first paint and reads persisted theme choice. Both Asap Sharp and JetBrains Mono are self-hosted; their licenses are in `licenses/`. Iconify Fluent icons are compiled into HTML at build time.

Astro's built-in CSP support emits hashes for its own scripts and styles. The policy includes `default-src 'self'`, same-origin `connect-src`, and self-hosted fonts; it does not allow unsafe inline scripts or style attributes. External same-origin scripts handle theme persistence, copy buttons, sorting, filters, and CSV export. The Rust server copies Astro's generated hashes to its HTTP CSP header and adds `frame-ancestors 'none'`.

## UX spec deviations

- Dynamic details use `/projects/view?slug=…`, `/jobs/view?id=…`, `/runs/view?id=…`, `/approvals/view?id=…`, and `/artifacts/view?id=…`, because static output cannot prerender unknown IDs and the Rust server must serve plain files.
- Mock-only mutations, including job creation, provider edits, resource upload, policy saves, and license import, are displayed as controls or CLI guidance but have no persistence. Only approval decisions update the in-memory mock adapter.
- Charts use compact native SVG bars and tables; full stacked area and scatter interactions await backend analytics endpoints. Values remain labeled by provenance.
- Mobile shows job status and approval evidence; decision buttons are hidden below 550px as a read-only preview, following the spec's Phase 2 mobile approval note.

## Known integration gaps

The live client view covers the core operational records and approval workflow, but does not yet reproduce every interaction in the mock design preview. Resources, licensing, settings, full review/diff display, provider configuration, and currency analytics still need server workflows. The core API exposes bound document content for approvals; complete diff presentation remains later work. The CLI still reads SQLite directly. Browser Playwright verification needs an environment that can launch Chromium; Rust HTTP integration, TypeScript unit, type check, lint and both static builds pass.
