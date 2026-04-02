# Code RAG

A RAG chatbot that answers questions about code repositories. Ingests all sibling project directories, parses Rust, Python, and TypeScript codebases with tree-sitter, extracts docstrings and call graphs, generates embeddings, and responds via Google Gemini. Intent classification routes queries to optimized retrieval strategies. Retrieval traces surface all sources with relevance scores — the system shows its work.

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
- Call graph extraction: direct + method calls enriched into embeddings
- Intent classification: cosine similarity against prototype query embeddings
- Query routing: declarative routing table maps intent → retrieval limits
- Retrieval traces: all 4 chunk types surfaced with relevance scores, sorted by relevance
- Quality harness: 43-query test dataset, recall@K, MRR, intent accuracy, latency — dual-run mode
- Baseline: recall@5 = 0.65, overview 1.00, implementation 0.70, comparison 0.75, relationship 0.38
- Incremental ingestion: SHA256 file hashing, skips unchanged files
- Shared `code-rag-engine` crate: pure algorithms compile to native + wasm32
- GitHub Pages demo: `standalone` feature runs full RAG pipeline in-browser (LLM generation optional)
- 192 tests, 0 warnings

## Known Limitations

- **Granularity**: Cannot search within functions or at file/module level
- **Relationships**: Call enrichment is probabilistic — no structured graph queries yet (relationship recall@5 = 0.38)
- **Exact match**: No keyword/BM25 search — exact identifier queries rely on semantic similarity alone

## Planned Features

See [project-vision.md](project-vision.md) and [development_plan.md](development_plan.md) for roadmap.

---

### Keywords

- **Language:** `Rust`
- **Architecture & Patterns:** `Layered Architecture (API/Store/Ingestion)` · `Trait-Based Abstraction (LanguageHandler)` · `Registry Pattern (OnceLock)` · `Three-Layer Pipeline (Parse→Reconcile→Orchestrate)` · `Router Pattern` · `Handler Pattern` · `Shared State (Arc)` · `Repository Pattern` · `DTO Pattern` · `Modular Design` · `Pipeline Pattern (Ingest→Embed→Store)` · `Visitor Pattern (WalkDir)` · `Error Propagation (thiserror)` · `Ephemeral Side-Channel Pattern` · `Declarative Routing Table` · `Scored Search API` · `ScoredChunk<T> (Generic Wrapper)` · `Retrieval Traces` · `Multi-Binary Crate (lib.rs extraction)` · `FlatChunk Centralization`
- **LLM & RAG:** `RAG (Retrieval-Augmented Generation)` · `LLM Integration` · `Google Gemini API` · `rig-core` · `Semantic Search` · `Chatbot` · `Intent Classification (Cosine Similarity)` · `Prototype Query Embeddings` · `Intent-Aware Retrieval` · `Cross-Type Source Ranking` · `Distance-to-Relevance Scoring` · `Retrieval Transparency`
- **Quality & Evaluation:** `Recall@K` · `MRR (Mean Reciprocal Rank)` · `Intent Accuracy` · `Latency Percentiles (p50/p95)` · `Dual-Run Evaluation (Classifier vs Ground-Truth)` · `Per-Intent Breakdown` · `Declarative Test Dataset` · `Substring File Matching` · `Dataset Freeze Policy` · `Baseline Regression Tracking`
- **Vector Database:** `LanceDB` · `FastEmbed` · `BGE Embeddings`
- **Code Analysis:** `Tree-sitter` · `AST Parsing` · `Code Chunking` · `Docstring Extraction` · `JSDoc Parsing` · `Multi-Language (Rust, Python, TypeScript)` · `Incremental Ingestion (SHA256)` · `Call Graph Extraction (AST-based)` · `Function Call Detection (Direct + Method)`
- **Web Framework:** `Axum` · `Leptos (WASM CSR)` · `Tower HTTP` · `CORS`
- **Async & Runtime:** `Tokio Runtime` · `Async Programming`
- **DevOps:** `Docker` · `Docker Compose` · `GitHub Pages (WASM)` · `Google OAuth2 (GIS)`
- **Rust Ecosystem:** `tracing` · `Error Handling (anyhow/thiserror)` · `Serde` · `clap (CLI)` · `chrono`