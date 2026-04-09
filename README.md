# Code RAG

A RAG chatbot that answers questions about code repositories. Ingests all sibling project directories, parses Rust, Python, and TypeScript codebases with tree-sitter, extracts docstrings and a persistent AST call graph, generates embeddings, and responds via Google Gemini. Intent classification routes queries to optimized retrieval strategies — including graph augmentation for relationship queries and per-comparator decomposition for comparison queries. Retrieval traces surface all sources with relevance scores — the system shows its work.

- [Executive Summary](docs/executive_summary.md)
- [Technical Summary](docs/technical_summary.md)

## Usage

Place this repo alongside the projects you want to index:

```
any-parent-folder/
├── code-rag/          ← this repo
├── project-a/
├── project-b/
└── ...
```

Ingestion walks all sibling directories — each becomes a separate project. The parent folder name doesn't matter.

1. `docker-compose -f docker-compose-ingest.yaml up`
2. `docker-compose up`

To clean, run `sh clean_docker.sh`.

## Development Roadmap

- [Development Log](development_log.md)
- [Development Plan](development_plan.md)
- [Project Vision](project-vision.md)

| Version | Date | Focus |
|---------|------|-------|
| **V0.1** | 2025-12-23 | MVP - Core engine |
| **V0.2** | 2026-01-01 | Docker deployment |
| **V0.3** | 2026-01-31 | Workspace restructuring |
| **V1.1** | 2026-02-04 | Schema foundation (UUID, content_hash, delete API) |
| **V1.2** | 2026-02-06 | LanguageHandler trait refactor |
| **V1.3** | 2026-02-06 | Incremental ingestion (SHA256, three-layer architecture) |
| **V1.4** | 2026-02-07 | TypeScript support (TSX grammar, JSDoc) |
| **V1.5** | 2026-02-07 | Docstring extraction (Rust, Python, TypeScript) |
| **V2.1** | 2026-02-07 | Inline call context (AST-based call extraction) |
| **V2.2** | 2026-02-08 | Intent classification + query routing (cosine similarity) |
| **V2.3** | 2026-02-08 | Retrieval traces (scored sources, cross-type ranking) |
| **Leptos Migration** | 2026-03-25 | WASM frontend (replace htmx/Askama with Leptos) |
| **GitHub Pages Demo** | 2026-03-25 | Shared engine crate + standalone WASM deployment |
| **V3.1** | 2026-04-02 | Retrieval test dataset (43 queries, 4 intent categories) |
| **V3.2** | 2026-04-02 | Recall measurement harness (recall@K, MRR, intent accuracy) |
| **V3.3** | 2026-04-03 | Baseline quality metrics (dual-run, per-intent breakdown) |
| **B1** | 2026-04-04 | Cross-encoder reranking (ms-marco-MiniLM-L-6-v2) |
| **B2** | 2026-04-04 | Hybrid BM25+semantic search infrastructure (disabled pending B3) |
| **B3** | 2026-04-05 | Declaration signatures + searchable_text + per-intent gating (recall@5 0.70→0.75) |
| **B4** | 2026-04-05 | Intent classifier 58%→74% (prototypes + k-NN + keyword pre-filter) |
| **B5** | 2026-04-06 | Dual-vector schema + per-intent ArmPolicy (bm25/rerank gating) |
| **C1** | 2026-04-09 | Graph RAG — call graph edges + 3-tier resolution + traversal (relationship 0.50→0.57) |
| **C2** | 2026-04-09 | Graph result protection — SOTA routing + soft reserve (relationship 0.57→0.60) |
| **C3** | 2026-04-09 | Comparison query decomposition — per-comparator RRF + project filter (comparison 0.62→0.65, aggregate 0.71→0.72) |

## Purpose

- To practice and demonstrate familiarity with
  - Rust
  - RAG
  - Chatbot Application
- To function as meta-project
  - To answer questions about sibling code repositories

## Tech Stack

- **Backend**: Rust + Axum
- **LLM**: Google Gemini (via rig-core)
- **Vector DB**: LanceDB
- **Embeddings**: FastEmbed
- **Frontend**: Leptos (Rust WASM, CSR)

---

## Guiding Principles

> **"Vertical slices, retrieval quality, code understanding"**

| Principle | Meaning |
|-----------|---------|
| **Vertical** | Build working end-to-end first, then deepen |
| **Retrieval** | Quality of retrieved context determines answer quality |
| **Understanding** | Goal is semantic code understanding, not just text search |

## Architecture

| Crate | Single Responsibility |
|-------|----------------------|
| `code-raptor` | Ingestion CLI — parsing, chunk extraction, data export |
| `code-rag-engine` | Shared algorithms — intent, context, scoring (pure, no I/O) |
| `code-rag-store` | Storage — embeddings, vector search (native) |
| `code-rag-types` | Shared types — no logic |
| `code-rag-chat` | Query API — retrieval, LLM, quality harness (2 binaries) |
| `code-rag-ui` | Leptos WASM SPA — chat interface (backend or standalone mode) |

## Current State

- Function-level chunking: 1 function/class → 1 vector (BGE-small, 384 dim)
- Supports Rust, Python, and TypeScript via tree-sitter AST parsing
- Docstrings extracted: `///` (Rust), `"""` (Python), `/** */` (TypeScript JSDoc)
- Declaration signatures extracted: functions + structs/enums/traits/interfaces/classes
- **Persistent call graph (Graph RAG)**: LanceDB scalar-only `call_edges` table (~3011 edges), 3-tier resolver (same-file → import-based → unique-global), AST scoped-identifier (`module::function()`) extraction
- **Test code exclusion at ingest** (3-level): directory `tests/`, filename `test_*.py` / `*.test.ts`, AST-walked `#[cfg(test)]` enclosing-mod detection (~24% chunk reduction)
- Intent classification: cosine similarity against prototype query embeddings
- Query routing: declarative routing table maps intent → retrieval limits
- **Two-stage retrieval**: hybrid (BM25 on `searchable_text` + vector) → cross-encoder reranking (ms-marco-MiniLM-L-6-v2), fused via N-ary RRF in shared `code-rag-engine::fusion`
- **Per-intent `ArmPolicy`**: per-intent `{body_vec, sig_vec, bm25, rerank}` gating (single source of truth, server + browser). Overview = hybrid+rerank; Implementation = rerank-only; Relationship = hybrid+rerank; Comparison = vector-only
- **Graph-augmented retrieval**: shared `code-rag-engine::graph` (`graph_augment`, `merge_graph_chunks`, `reserve_graph_slots`, `detect_direction`). Two protection paths: SOTA routing for explicit-direction queries ("what calls X / called by") partitions graph chunks **out** of the reranker entirely; soft reserve for ambiguous-direction over-retains the code arm by `+5` and rescues demoted graph chunks. Mirrored line-for-line in WASM standalone
- **Comparison query decomposition** (`code-rag-engine::comparison`): regex extracts ≥2 comparators → per-comparator body-vec sub-searches (comparator name prepended to original query) → vote-based dominant-project filter → RRF fusion → max-of-natural rescoring so RRF outputs compete with non-code arms. Mirrored in WASM standalone
- **Dual-vector schema**: nullable `signature_vector` column populated at ingest (shipped OFF after 8-config space sweep; column retained for future experiments)
- Intent classifier: prototype cosine similarity + k-NN (k=3) weighted voting + Comparison keyword pre-filter with adversarial guards — **74% accuracy** (was 58%)
- Retrieval traces: all 4 chunk types surfaced with relevance scores, sorted by relevance
- Quality harness: 81-query cleaned test dataset (73 recall-scoreable), recall@K, MRR, intent accuracy, latency — dual-run mode
- Post-C3 (composite `ArmPolicy`, classifier routing, 81-case dataset): recall@5 = **0.72** aggregate · overview 0.80 · implementation 0.76 · relationship 0.60 · comparison 0.65 · recall@10 = 0.76 · MRR = 0.71
- Incremental ingestion: SHA256 file hashing, skips unchanged files
- Shared `code-rag-engine` crate: pure algorithms compile to native + wasm32
- GitHub Pages demo: `standalone` feature runs full RAG pipeline in-browser (LLM generation optional)

## Known Limitations

- **Granularity**: Cannot search within functions or at file/module level
- **Comparison short-identifier ceiling**: Two stubborn pre-C3 failures (`comp-retriever-generator`, `b4-comp-retriever-api`) remain — BGE-small produces noisy vectors for bare hyphenated identifiers (`retriever`, `generator`), and the C3 regex extracts comparators only from explicit "compare X and Y / X vs Y" phrasings. Gated on a future embedder upgrade (BGE-base / jina-code) or MMR fallback
- **Classifier**: No longer the dominant bottleneck post-B4 (+2pp classifier→GT gap on recall@5). Implementation and Relationship classification still below targets (70% / 53%)

## Planned Features

See [project-vision.md](project-vision.md) and [development_plan.md](development_plan.md) for roadmap.

---

### Keywords

- **Language:** `Rust`
- **Architecture & Patterns:** `Layered Architecture (API/Store/Ingestion)` · `Trait-Based Abstraction (LanguageHandler)` · `Registry Pattern (OnceLock)` · `Three-Layer Pipeline (Parse→Reconcile→Orchestrate)` · `Router Pattern` · `Handler Pattern` · `Shared State (Arc)` · `Repository Pattern` · `DTO Pattern` · `Modular Design` · `Pipeline Pattern (Ingest→Embed→Store)` · `Visitor Pattern (WalkDir)` · `Error Propagation (thiserror)` · `Ephemeral Side-Channel Pattern` · `Declarative Routing Table` · `Scored Search API` · `ScoredChunk<T> (Generic Wrapper)` · `Retrieval Traces` · `Multi-Binary Crate (lib.rs extraction)` · `FlatChunk Centralization`
- **LLM & RAG:** `RAG (Retrieval-Augmented Generation)` · `Graph RAG` · `Call Graph Augmentation` · `Graph-Augmented Retrieval` · `SOTA Routing (Reranker Bypass)` · `Soft Reserve` · `Comparison Query Decomposition` · `Per-Comparator RRF Fusion` · `Sub-Query Expansion` · `Vote-Based Project Filter` · `Max-of-Natural Rescoring` · `LLM Integration` · `Google Gemini API` · `rig-core` · `Semantic Search` · `Chatbot` · `Intent Classification (Cosine Similarity)` · `Prototype Query Embeddings` · `k-NN Prototype Voting` · `Keyword Pre-Filter (adversarial-guarded)` · `Intent-Aware Retrieval` · `Per-Intent Gating (ArmPolicy)` · `Two-Stage Retrieval` · `Cross-Encoder Reranking` · `Hybrid Search (BM25 + Dense)` · `RRF Fusion` · `Dual-Vector Schema` · `Declaration Signatures` · `searchable_text (IR field boosting)` · `camelCase Splitting (index-time)` · `Cross-Type Source Ranking` · `Distance-to-Relevance Scoring` · `Retrieval Transparency`
- **Quality & Evaluation:** `Recall@K` · `MRR (Mean Reciprocal Rank)` · `Intent Accuracy` · `Latency Percentiles (p50/p95)` · `Dual-Run Evaluation (Classifier vs Ground-Truth)` · `Per-Intent Breakdown` · `Declarative Test Dataset` · `Substring File Matching` · `Dataset Freeze Policy` · `Baseline Regression Tracking` · `Space Search (per-intent ArmPolicy sweep)` · `Adversarial Test Cases` · `Held-out Classifier Eval`
- **Vector Database:** `LanceDB` · `LanceDB FTS` · `Scalar-Only LanceDB Table (call_edges)` · `BM25` · `FastEmbed` · `BGE Embeddings` · `ms-marco-MiniLM-L-6-v2 (ONNX)`
- **Code Analysis:** `Tree-sitter` · `AST Parsing` · `Code Chunking` · `Docstring Extraction` · `JSDoc Parsing` · `Multi-Language (Rust, Python, TypeScript)` · `Incremental Ingestion (SHA256)` · `Call Graph Extraction (AST-based)` · `Function Call Detection (Direct + Method)` · `Call Edge Resolution (3-tier)` · `Import-Based Symbol Resolution` · `Scoped Identifier Extraction` · `Test Code Exclusion (cfg(test) AST walk)`
- **Web Framework:** `Axum` · `Leptos (WASM CSR)` · `Tower HTTP` · `CORS`
- **Async & Runtime:** `Tokio Runtime` · `Async Programming`
- **DevOps:** `Docker` · `Docker Compose` · `GitHub Pages (WASM)` · `Google OAuth2 (GIS)`
- **Rust Ecosystem:** `tracing` · `Error Handling (anyhow/thiserror)` · `Serde` · `clap (CLI)` · `chrono`