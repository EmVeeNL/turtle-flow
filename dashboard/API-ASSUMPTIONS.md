# Local API v1

The daemon serves the built dashboard and JSON API on `http://127.0.0.1:<port>`. The chosen port is in `<TURTLE_HOME>/run/api.port` (0600). `turtle dashboard` requires an interactive terminal, creates a 60-second single-use full-scope code, and opens `/login#code=…`. `turtle dashboard --phone` issues a read-only code for `https://<tailnet-host>/login#code=…` after checking the daemon's configured host and Tailscale Serve status. The fragment never reaches the server. `/api/v1/session` exchanges the code for an in-memory 12-hour sliding session. Loopback cookies are `HttpOnly; SameSite=Strict; Path=/`; tailnet cookies also set `Secure`.

Every request needs a configured Host: loopback hosts above, or the daemon's exact validated `--tailnet-host` / auto-detected Tailscale DNS name. Mutating methods also need an exact same-origin `Origin` and `X-Turtle-Request: 1`. Only `/login`, `/login.js`, `/api/v1/session`, and `/api/v1/health` are public. No CORS headers are sent. Read-only sessions may use GET and HEAD only; other methods return `403` with code `read_only_session`. `GET /api/v1/me` returns `{ "readonly": boolean }`. Errors are JSON: `{ "error": { "code": "not_found|conflict|unauthorized|unavailable|bad_request|invalid_origin|invalid_host|invalid_code|read_only_session|method_not_allowed", "message": "…" } }`.

| Method | Path | Result |
| --- | --- | --- |
| GET | `/api/v1/health` | Public `{status:"ok",tailnet_host:string|null}`; the host is used by the CLI to verify daemon configuration |
| GET | `/api/v1/me` | Current session scope `{readonly:boolean}` |
| POST | `/api/v1/session` | `{code}` → session cookie; expired and replayed codes return 401 |
| GET | `/api/v1/overview` | `DashboardData`: projects, jobs, pending approvals, recent runs and events, artifacts, tasks, providers, budgets. Resources are empty until resource ingestion exists. |
| GET | `/api/v1/projects/{slug}` | Project with its jobs, or 404 |
| GET | `/api/v1/jobs/{key}?include=tasks,artifacts,approvals,usage,events` | Job with the requested related arrays or 404. The base job always includes `version`. |
| GET | `/api/v1/approvals/{id}` | Full binding, echoed `subject_hash`, required flags, and the bound redacted documents' stored content |
| POST | `/api/v1/approvals/{id}/decisions` | `{decision:"approve"|"changes"|"reject",subject_hash,comment,acknowledged_flags}`. Goes through `ApprovalService::decide` as a user on channel `dashboard`. Hash mismatch or stale binding returns 409 and decides nothing. Comments are required for changes and reject. |
| POST | `/api/v1/jobs/{key}/commands` | `{command:"start"|"pause"|"resume"|"cancel",expected_version,idempotency_key}`. A key is recorded atomically with the transition and the newest 1024 keys are retained. |
| GET | `/api/v1/runs/{id}` | Run with usage provenance or 404 |
| GET | `/api/v1/runs/{id}/transcript?offset=0&limit=100` | Paged redacted lines and `next_offset`; no transcript yields an unavailable result |
| GET | `/api/v1/artifacts/{id}` | Artifact revision metadata and stored UTF-8 content (null for binary data), or 404 |
| GET | `/api/v1/analytics/usage` | Job-level token totals with provenance |
| GET | `/api/v1/events/stream?project=…&job=…&lastEventId=…` | SSE from the hash-chained event log; `id` is decimal event `seq`. `Last-Event-ID` works too. Query cursor takes precedence. |

`null` means unavailable or not yet recorded: no invented estimate, provider version, price, delivery timestamp, gate pass rate, cost, or token count. Run and job usage carry `reported`, `estimated`, or `unavailable` provenance. An unmetered run makes the job total unavailable even if other runs have measured usage. The read API does not yet include resource ingestion, capability discovery, or currency analytics.

The default dashboard build uses the mock adapter. Build with `PUBLIC_API_ADAPTER=real npm run build` to serve live data. The real build uses a client view over these endpoints while the more detailed mock layout remains the design preview. Astro's generated CSP hashes are carried into the HTTP response header so the island loader can run under a strict policy.
