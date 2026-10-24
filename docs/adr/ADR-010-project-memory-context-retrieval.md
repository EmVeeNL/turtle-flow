# ADR-010: Project Memory and Context Retrieval

- **Status:** Accepted
- **Date:** 2026-10-24

## Context

The brief proposes a small local LLM that makes Turtle Flow "increasingly knowledgeable" about projects. Agents re-learn conventions in every session (P8), and context size drives cost.

## Problem Statement

How should project knowledge be captured, retrieved and kept fresh so that context packages are small and relevant — and what role, if any, should a local LLM play?

## Critical evaluation of the local-LLM idea

- **An LLM does not learn from use.** Inference does not update weights. "Getting smarter over time" requires either (a) storing knowledge and retrieving it into prompts, or (b) fine-tuning — expensive, slow to update, hard to invalidate when code changes, and risky (memorizing stale or wrong facts).
- **Most knowledge is structured and deterministic:** commands, conventions, file layout, past errors and their fixes, decisions. Storing them as data with provenance is cheaper, auditable, and invalidatable.
- **Where a local model helps:** summarizing long artifacts/logs cheaply, classifying task categories, reranking retrieved chunks, computing embeddings, offline/air-gapped scenarios.
- **Where it hurts:** model management (downloads of several GB, GPU/CPU contention with builds), quality variance, another component to secure and support.

## Decision

1. **Retrieval-first Project Intelligence Layer**: structured `knowledge_items` (facts, conventions, decisions, lessons, repo map, summaries) with provenance, status and validity; plus resources and artifacts — all indexed in **SQLite FTS5** (BM25) with metadata filters.
2. **Context packaging is deterministic** with per-role budgets and inclusion reasons recorded in a manifest ([TOKEN-INTELLIGENCE.md §10](../TOKEN-INTELLIGENCE.md#10-context-size-optimization)).
3. **Invalidation:** items record `scope_paths`, `valid_at_commit` and file content hashes; changes to referenced files mark items `stale`; stale items are excluded until re-validated (by user or a summarizer run).
4. **Agent-proposed knowledge requires approval** before use.
5. **Embeddings / vector search (sqlite-vec or in-process ONNX embeddings) and local LLM via Ollama/llama.cpp are optional, post-MVP**, behind capability flags, used only for: embeddings, reranking, summarization, classification. **No fine-tuning** in the roadmap.

## Considered Alternatives

| Alternative | Assessment |
|---|---|
| Fine-tune a local model per project | Expensive, stale quickly, unverifiable, hard to remove wrong knowledge |
| Vector DB first (Qdrant/Chroma/LanceDB) | Extra service/dependency; semantic search adds value mainly for prose, while code questions are often lexical (identifiers) |
| Tantivy instead of FTS5 | Better ranking features, but second index engine; FTS5 is enough and transactional with our DB |
| Send everything (full resources, whole files) | Token waste; injection surface |
| Let agents retrieve themselves (they have file tools) | They do, for code; Turtle Flow retrieval is for knowledge outside the repo and for steering |

## Rationale

Structured knowledge with provenance is cheaper, explainable and safely invalidatable. FTS5 is zero-dependency. Local models can be added where measurable benefit exists.

## Consequences

- Knowledge quality depends on capturing it (user input, lessons from fix loops, approved proposals).
- Lexical search misses paraphrases; acceptable initially.

## Risks

- Stale knowledge misleads agents.
- Knowledge store grows noisy.

## Mitigations

- Hash-based staleness; status lifecycle; usage tracking (items never retrieved or frequently contradicted are flagged for retirement).
- Evaluation: measure first-pass gate rate with/without knowledge injection (A/B on eval suite) before expanding.

## Related ADRs

[ADR-013](ADR-013-resource-artifact-management.md), [ADR-008](ADR-008-token-intelligence-budgets.md), [ADR-018](ADR-018-model-routing-and-evaluation.md)
