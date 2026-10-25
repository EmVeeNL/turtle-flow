# API assumptions

The Rust server API is not implemented by this frontend. Set `PUBLIC_API_ADAPTER=real` at build time to select the fetch adapter. The default build uses deterministic mock data. Real adapter requests use same-origin credentials and JSON.

| Method | Endpoint | Assumed response |
| --- | --- | --- |
| GET | `/api/v1/overview` | `DashboardData` aggregate: projects, jobs, approvals, runs, artifacts, tasks, resources, events, providers |
| GET | `/api/v1/projects/{slug}` | `Project` or 404 |
| GET | `/api/v1/jobs/{key}` | `Job` or 404; include related records in the overview aggregate or extend with `?include=tasks,artifacts,approvals,usage,events` |
| GET | `/api/v1/approvals/{id}` | `Approval` with full binding and `subject_hash` |
| POST | `/api/v1/approvals/{id}/decisions` | Body `{decision,subject_hash,comment}`; response `{ok,error?}`; mismatch must reject without deciding |
| GET | `/api/v1/runs/{id}` | `AgentRun` |
| GET | `/api/v1/events` | SSE messages whose `data` is the typed `Event` JSON, including monotonic `id` and `seq` |

The SSE client retries with exponential backoff capped at 30 seconds. Browser `EventSource` cannot set a custom `Last-Event-ID` request header, so reconnects add `?lastEventId=<id>`; the Rust server should accept this cursor as the equivalent. Native EventSource also sends its own `Last-Event-ID` header when reconnecting the same instance. The mock transport uses the same cursor semantics.

The backend should return 409 for a changed approval binding, 404 for unknown IDs, and 503 when the daemon cannot answer. For a static Rust bundle, serve `/jobs/view`, `/approvals/view`, `/artifacts/view`, `/runs/view`, and `/projects/view` from their built `index.html` files. IDs come from query parameters; no build-time enumeration is required.

Data shapes in `src/lib/api/types.ts` mirror the domain nouns in `docs/DOMAIN-MODEL.md` and `docs/DATA-MODEL.md`. Dates are ISO 8601 strings; token counts are integers. `UsageProvenance` must be sent with each usage value so the UI can keep measured, estimated, and unavailable states distinct.
