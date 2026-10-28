# Turtle Flow dashboard

The default build is a design preview backed by mock data. The `PUBLIC_API_ADAPTER=real` build loads live projects, jobs, approvals, runs, usage and events from the local daemon. Its client view shows unavailable values as unknown; the richer static layouts remain available in the mock preview.

## Run and verify

```sh
cd dashboard
pnpm install
pnpm run dev
pnpm run check
pnpm run lint
pnpm test
ppnpm run build
pnpm exec playwright install chromium
pnpm run test:e2e
```

The static bundle is in `dist/`. For live use, run `PUBLIC_API_ADAPTER=real pnpm run build`, start `turtle up --agent demo` (or a real provider), then run `turtle dashboard` in an interactive terminal. The daemon serves the bundle on its loopback port; `/login#code=…` exchanges the one-use code for a session cookie. For a Tailscale phone session, run `turtle dashboard --phone`; it asks before adding a Serve mapping and prints a QR code containing a read-only, one-time login link. Build with the real adapter. The daemon continues to listen on loopback; Tailscale terminates HTTPS and forwards to the local API. The implemented endpoint contract is in [API-ASSUMPTIONS.md](../../docs/dashboard/API-ASSUMPTIONS.md).

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
- Phone sessions are read-only in the API and UI at every viewport; the live dashboard hides command and decision controls when `/api/v1/me` reports `readonly: true`. The mock preview keeps its existing below-550px read-only behavior. Bound JSON documents are pretty-printed and wrapped for phone widths.

## Known integration gaps

The live client uses the mock screen structure for overview, project and job details, runs, approvals, analytics, and provider observations. Full sessions can add a validated Git project, create and optionally start jobs, and set job/project/global token budgets. Read-only sessions hide those controls and the API guard rejects writes. Approval documents are rendered from bound content; stored patch artifacts open in a line-oriented diff view when available. Resources remain an explicit empty state; settings and licensing are read-only. Currency costs, usage time series, provider discovery/configuration, repository settings provenance, and licensing data remain unknown or unavailable because the API does not provide those values. The CLI still reads SQLite directly. No working Tailscale status/Serve environment was available; Serve command syntax follows current Tailscale docs, and stand-in CLI tests cover the invocation.
