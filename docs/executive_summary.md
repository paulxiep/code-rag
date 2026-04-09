# Portfolio RAG Chat — Executive Summary

## What It Is

A RAG (Retrieval-Augmented Generation) chatbot that answers questions about code repositories. Parses Rust, Python, and TypeScript codebases with tree-sitter, extracts docstrings and call graphs, generates embeddings with FastEmbed, stores in LanceDB, and responds via Google Gemini. Intent classification routes queries to optimized retrieval strategies, and retrieval traces surface all sources with relevance scores.

## Why It Matters

- **Portfolio showcase**: Demonstrates Rust, RAG architecture, and chatbot development skills
- **Meta-project**: Can answer questions about itself and other portfolio projects
- **Code understanding**: Semantic search over function-level code chunks

## Key Features

- **Multi-language parsing**: Rust, Python, and TypeScript via tree-sitter AST queries
- **Docstring extraction**: `///` (Rust), `"""` (Python), `/** */` (TypeScript JSDoc) — enriches embeddings and LLM context
- **Persistent call graph (Graph RAG)**: LanceDB scalar-only `call_edges` table (~3011 edges), 3-tier resolver (same-file → import-based → unique-global), graph traversal (callers/callees/path) augments retrieval at query time. AST scoped-identifier (`module::function()`) extraction added
- **Test code exclusion at ingest**: 3-level — directory `tests/`, filename `test_*.py` / `*.test.ts`, AST-walked `#[cfg(test)]` enclosing-mod detection. Removed ~24% of chunks (3772 → 2861)
- **Intent classification**: Cosine similarity against prototype query embeddings + k-NN (k=3) weighted voting + keyword pre-filter with adversarial guards — 74% accuracy (semantic, not keyword-based)
- **Query routing**: Declarative routing table maps intent (overview, implementation, relationship, comparison) to per-type retrieval limits
- **Two-stage retrieval**: Hybrid BM25 (on `searchable_text`) + dense vector search fused with RRF → cross-encoder reranking (`ms-marco-MiniLM-L-6-v2`, ONNX)
- **Per-intent `ArmPolicy`**: `{body_vec, sig_vec, bm25, rerank}` gates per intent — single source of truth shared server + browser
- **Graph result protection**: SOTA routing partitions graph hits **out** of the reranker entirely for explicit-direction queries ("what calls X / called by"); soft-reserve over-retains the code arm by `+5` and rescues demoted graph chunks for ambiguous-direction. Identical logic in server (`src/engine/retriever.rs`) and WASM standalone (`crates/code-rag-ui/src/standalone_api.rs`)
- **Comparison query decomposition**: Regex extracts ≥2 comparators → per-comparator body-vec sub-searches (comparator name prepended to original query) → vote-based dominant-project filter → RRF fusion → max-of-natural score rescoring (so RRF outputs compete on equal terms with distance-converted non-code arms). Pure-Rust, wasm32-compatible (`code-rag-engine::comparison`)
- **Declaration signatures**: Functions + structs/enums/traits/interfaces/classes extracted at ingest; power `searchable_text` (identifier 2× boost + camelCase split + signature + docstring) as the FTS target
- **Dual-vector schema**: Nullable `signature_vector` column (shipped OFF after empirical space sweep; column retained)
- **Retrieval traces**: All 4 chunk types surfaced with relevance scores, sorted by relevance — the system shows its work
- **Quality harness**: 81-query cleaned test dataset (73 recall-scoreable, +48 B4 held-out classifier cases) with automated recall@K, MRR, intent accuracy, and latency measurement — dual-run mode isolates classifier vs. retrieval quality
- **Multi-binary crate**: `src/lib.rs` extraction enables `code-rag-harness` second binary alongside the main server — shared library, independent entry points
- **Centralized chunk flattening**: `FlatChunk` + `flatten()` — single source of truth for both API responses and harness evaluation
- **Incremental ingestion**: SHA256 file hashing skips unchanged files for fast re-indexing
- **4 chunk types**: Code functions, README files, Crate metadata, Module docs
- **Trait-based language abstraction**: Add new languages by implementing `LanguageHandler` trait
- **Vector search**: LanceDB with FastEmbed (BGE-small-en-v1.5, 384 dimensions)
- **LLM integration**: Google Gemini via rig-core (optional — retrieval works without API key)
- **Web UI**: Leptos WASM SPA (Rust compiled to WebAssembly)
- **GitHub Pages demo**: Full RAG pipeline runs in-browser via `standalone` feature — no backend needed
- **Shared engine**: `code-rag-engine` crate compiles to both native and wasm32

## Quick Start

```bash
# 1. Ingest repositories
docker-compose -f docker-compose-ingest.yaml up

# 2. Run query server
docker-compose up
```

Open http://localhost:3000 for the chat interface.

## Current Quality Metrics (post-C3)

Measured against the 81-case cleaned test dataset (73 recall-scoreable), composite per-intent `ArmPolicy`, classifier routing (commit ee22398, label `c3_post8`):

| Metric | Classifier |
|--------|:---------:|
| recall@5 (aggregate) | 0.72 |
| recall@10 (aggregate) | 0.76 |
| MRR | 0.71 |
| Intent accuracy (97-case held-out corpus) | 74% |

Per-intent recall@5: overview 0.80, implementation 0.76, relationship 0.60, comparison 0.65.

The lifts came from the retrieval infrastructure: cross-encoder reranking (B1), hybrid BM25+dense with RRF (B2), signature-aware `searchable_text` (B3), and a tuned per-intent `ArmPolicy` (B5). B4 raised classifier accuracy 58%→74%, closing the classifier-vs-GT retrieval gap to ~2pp — classification is no longer the dominant bottleneck. A dual-vector experiment (`signature_vector`) was tested and rejected (short-text geometry + sparse-arm RRF penalty); the column remains for future work. Track C added Graph RAG (C1: relationship 0.50→0.57), graph result protection (C2: relationship 0.57→0.60), and comparison query decomposition (C3: comparison 0.62→0.65, aggregate 0.71→0.72, MRR 0.69→0.71, recall@10 → 0.76).

## Current State

310 tests, 0 warnings:
- `code-raptor`: Ingestion CLI — trait-based language handlers, incremental ingestion, docstring + signature + call extraction, **3-tier call edge resolution**, **3-level test code exclusion**, data export (incl. IDF tables + signature embeddings + call edges)
- `code-rag-engine`: Shared algorithms — intent classification (k-NN + pre-filter + comparator extraction), context building, scoring, N-ary `rrf_fuse`, `ArmPolicy`, **`graph` (`graph_augment`, `merge_graph_chunks`, `reserve_graph_slots`, `detect_direction`)**, **`comparison` (`fuse_comparator_lists`)** (compiles to native + wasm32)
- `code-rag-store`: Embedder + VectorStore + Reranker — scored search API, hybrid BM25+vector search via LanceDB FTS, nullable `signature_vector` column, **scalar-only `call_edges` table** + `get_chunks_by_ids`
- `code-rag-types`: Shared types — UUID chunk IDs, content hashes, nullable docstrings, **`CallEdge` + `ExportEdge`**
- `code-rag-chat`: Query API — retrieval (graph augmentation + comparison decomposition pre-branch), LLM, quality harness, serves WASM UI
- `code-rag-ui`: Leptos WASM SPA — default mode calls backend API, standalone mode runs full RAG pipeline in-browser (mirrors graph augmentation + comparison decomposition line-for-line)

## Technology

- **Language**: Rust
- **Web Framework**: Axum 0.8
- **LLM**: Google Gemini (rig-core 0.27)
- **Vector Database**: LanceDB
- **Embeddings**: FastEmbed (BGE-small-en-v1.5)
- **Code Parsing**: tree-sitter (Rust, Python, TypeScript/TSX)
- **Frontend**: Leptos 0.8 (Rust WASM, CSR)
- **Deployment**: Docker (local) + GitHub Pages (static demo)
