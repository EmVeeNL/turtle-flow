# ADR-013: Resource and Artifact Management

- **Status:** Accepted
- **Date:** 2026-10-24

## Context

Jobs consume **resources** (user inputs: docs, PDFs, OpenAPI, fixtures, screenshots) and produce **artifacts** (spec, plan, review, transcripts…). Approvals bind to artifact hashes (ADR-006). Resources must not be sent wholesale to agents (brief §10).

## Problem Statement

How are resources and artifacts stored, versioned, hashed, secured and selected for context?

## Decision

1. **Content-addressed blob store** at `~/.turtle/blobs/sha256/<2>/<2>/<hash>` (0600), zstd-compressed for large text, reference-counted for GC; metadata in SQLite.
2. **Artifacts:** logical artifact + immutable revisions (`rev`, `sha256`, `schema_id`, author). Structured artifacts are canonical JSON (RFC 8785) with a Markdown rendering generated on demand.
3. **Resources:** logical resource + immutable versions; attachments to project/job/task referencing a version or `latest` (resolved and recorded per context manifest).
4. **Trust levels** `trusted | internal | untrusted` set by the user; agent-produced content is never `trusted`.
5. **Extraction pipeline** (async, off the writer): Markdown (heading-aware chunks), text/code (line-window chunks), JSON/YAML (OpenAPI operation-level chunks, Could), PDF text in a sandboxed subprocess with timeout (Should). Chunks indexed in FTS5.
6. **Selection:** pinned items first, then FTS-ranked chunks filtered by scope and trust, within per-role context budgets; never-include rules and secret redaction applied; manifest records reason and estimated tokens per item.
7. **URL resources** are snapshotted at attach time; never fetched during context assembly.

## Considered Alternatives

| Alternative | Assessment |
|---|---|
| Store blobs in SQLite | Simpler backup, but DB bloat and slower vacuum; files are better for large binaries |
| Store artifacts in the Git repository | Pollutes project history; mixes Turtle Flow metadata with product code; cross-job artifacts awkward |
| Mutable resources (overwrite) | Breaks reproducibility and approval binding |
| Object storage (S3) locally | Unnecessary in MVP; the store interface allows S3 in Phase 2 |

## Rationale

Content addressing gives dedup, integrity checks and natural immutability — exactly what approval binding and audit need.

## Consequences

- Backup must include both DB and blob directory (export tool handles both).
- GC must be careful (only after ref_count reaches zero and retention passed).

## Risks

- Parser vulnerabilities in PDF extraction.
- Large resources inflate storage.

## Mitigations

- Out-of-process extraction with memory/time limits; no execution of embedded content.
- Size limits per resource (default 50 MB), quotas per project, retention for transcripts.

## Related ADRs

[ADR-004](ADR-004-sqlite-persistence-event-logging.md), [ADR-006](ADR-006-supervised-autonomy-approvals.md), [ADR-010](ADR-010-project-memory-context-retrieval.md), [ADR-012](ADR-012-agent-execution-security.md)
