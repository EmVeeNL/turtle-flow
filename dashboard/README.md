# Turtle Flow dashboard

Frontend preview for the Turtle Flow daemon. The default adapter is mocked, so every screen works without a backend. The mock data is illustrative and follows the supplied dashboard screenshots (`payments-api`, `API-42`, and related jobs).

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

The static bundle is in `dist/`. `npm run preview` serves it locally. Set `PUBLIC_API_ADAPTER=real` during build to point the fetch adapter at `/api/v1/*` once a Rust server implements the assumptions in [API-ASSUMPTIONS.md](API-ASSUMPTIONS.md).

## Structure and decisions

- `src/pages/` contains the twelve screens plus list and query-string detail routes.
- `src/layouts/AppShell.astro` contains the shared sidebar, project switcher, top bar, and theme control.
- `src/components/*.astro` renders static tables, badges, budgets, estimates, hashes, and steppers.
- Preact islands handle the command palette, approval decision, diff, live log virtualization, and query-string detail lookup. Preact was chosen because these controls share small local states and keyboard events; most dashboard HTML remains static Astro output. Islands hydrate only on pages that use them.
- `src/lib/api/` defines typed domain records and an adapter interface; `mock.ts` is the default, `real.ts` is the same-origin stub. The reconnecting SSE client is separate from its mock and browser transports.
- `public/theme.js` runs before first paint and reads persisted theme choice. Both Asap Sharp and JetBrains Mono are self-hosted; their licenses are in `licenses/`. Iconify Fluent icons are compiled into HTML at build time.

Astro's built-in CSP support emits hashes for its own scripts and styles. The policy includes `default-src 'self'`, same-origin `connect-src`, and self-hosted fonts; it does not allow unsafe inline scripts or style attributes. External same-origin scripts handle theme persistence, copy buttons, sorting, filters, and CSV export. Avoid adding raw `style=` or inline handlers. The Rust server may strengthen the policy in an HTTP header if it carries forward Astro's generated script/style hashes.

## UX spec deviations

- Dynamic details use `/projects/view?slug=…`, `/jobs/view?id=…`, `/runs/view?id=…`, `/approvals/view?id=…`, and `/artifacts/view?id=…`, because static output cannot prerender unknown IDs and the Rust server must serve plain files.
- Mock-only mutations, including job creation, provider edits, resource upload, policy saves, and license import, are displayed as controls or CLI guidance but have no persistence. Only approval decisions update the in-memory mock adapter.
- Charts use compact native SVG bars and tables; full stacked area and scatter interactions await backend analytics endpoints. Values remain labeled by provenance.
- Mobile shows job status and approval evidence; decision buttons are hidden below 550px as a read-only preview, following the spec's Phase 2 mobile approval note.

## Known integration gaps

The daemon API, persistence, authentication, and real SSE endpoint are not part of this frontend task. The real adapter is a contract stub until the Rust server implements it. Some workflow controls show the intended command or review route rather than performing a backend mutation.
