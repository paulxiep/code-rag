# Portfolio RAG Chat — Technical Summary

## Architecture

```
┌───────────────────────────────────────────────────────────────────┐
│                        Cargo Workspace                            │
├───────────────────────────────────────────────────────────────────┤
│                                                                   │
│  ┌─────────────────┐   ┌─────────────────┐   ┌────────────────┐  │
│  │   code-raptor   │   │  code-rag-chat  │   │  code-rag-ui   │  │
│  │   (Indexing)    │   │  (Query API)    │   │ (Leptos WASM)  │  │
│  │                 │   │                 │   │                │  │
│  │  - CLI          │   │  - Axum server  │   │  - CSR SPA     │  │
│  │  - tree-sitter  │   │  - LLM client   │   │  - standalone  │  │
│  │  - walkdir      │   │  - Harness bin  │   │    mode        │  │
│  └────────┬────────┘   └────────┬────────┘   └───────┬────────┘  │
│           │                     │                     │          │
│           │              ┌──────┴──────┐              │          │
│           │              ▼             │              │          │
│           │    ┌─────────────────┐     │              │          │
│           │    │ code-rag-engine │     │              │          │
│           │    │ (Pure Algos)    │◄────┼──────────────┘          │
│           │    │ - intent        │     │  compiles to             │
│           │    │ - context       │     │  native + wasm32         │
│           │    │ - retriever     │     │                          │
│           │    │ - FlatChunk     │     │                          │
│           │    └────────┬────────┘     │                          │
│           │             │              │                          │
│           ▼             ▼              ▼                          │
│  ┌──────────────────────────────────────────┐                    │
│  │            code-rag-store                │                    │
│  │  - Embedder (FastEmbed)                  │                    │
│  │  - VectorStore (LanceDB)                 │                    │
│  └─────────────────┬────────────────────────┘                    │
│                    │                                             │
│                    ▼                                             │
│  ┌──────────────────────────────────────────┐                    │
│  │            code-rag-types                │                    │
│  │  - CodeChunk, ReadmeChunk                │                    │
│  │  - CrateChunk, ModuleDocChunk            │                    │
│  └──────────────────────────────────────────┘                    │
│                                                                   │
└───────────────────────────────────────────────────────────────────┘
```

## Crate Responsibilities

| Crate | Purpose | Key Files |
|-------|---------|-----------|
| `code-raptor` | Ingestion CLI — tree-sitter parsing, language handlers, incremental ingestion, data export | `ingestion/`, `export.rs`, `main.rs` |
| `code-rag-engine` | Shared algorithms — intent classification, context building, scoring (pure, no I/O, compiles to wasm32) | `intent.rs`, `context.rs`, `config.rs`, `retriever.rs` |
| `code-rag-store` | Embedder (FastEmbed) + VectorStore (LanceDB) with scored search API | `embedder.rs`, `vector_store.rs` |
| `code-rag-types` | Shared types — CodeChunk, ReadmeChunk, etc. with UUID, content_hash | `lib.rs` |
| `code-rag-chat` | Query API — retrieval, LLM, quality harness, serves WASM UI | `api/`, `engine/`, `harness/`, `bin/harness.rs` |
| `code-rag-ui` | Leptos WASM SPA — chat interface (default: backend API, standalone: in-browser RAG) | `components/`, `standalone_api.rs` |

## Query Pipeline

```
User Query
    │
    ▼
┌─────────────────┐
│   Axum Router   │  POST /api/chat
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│    Embedder     │  embed_one(query) → Vec<f32> (384-dim, ~5ms)
└────────┬────────┘
         │
         ├──────────────────────────┐
         ▼                          ▼
┌─────────────────┐      ┌──────────────────┐
│   Classifier    │      │    Retriever     │
│  cosine sim vs  │─────▶│  searches 4      │
│  prototype emb. │route │  tables with     │
└─────────────────┘      │  intent limits   │
                         └────────┬─────────┘
                                  │
                    ┌─────────────┴─────────────┐
                    ▼                           ▼
          ┌─────────────────┐         ┌─────────────────┐
          │ Context Builder │         │  Source Builder  │
          │ chunks → markdown│        │ ScoredChunk →   │
          │ (ignores scores)│         │ SourceInfo      │
          └────────┬────────┘         └────────┬────────┘
                   │                           │
                   ▼                           │
          ┌─────────────────┐                  │
          │    Generator    │  Gemini          │
          └────────┬────────┘                  │
                   │                           │
                   ▼                           ▼
          JSON/HTML Response { answer, sources, intent }
```

## Vector Schema (4 Tables)

| Table | Content | Embedding Input |
|-------|---------|-----------------|
| `code_chunks` | Functions, classes, structs | `identifier (language) + docstring + code + calls` |
| `readme_chunks` | README.md files | `Project: name + content` |
| `crate_chunks` | Cargo.toml metadata | `Crate: name + description + dependencies` |
| `module_doc_chunks` | Module-level docs (`//!`) | `Module: name + doc_content` |

## Ingestion Pipeline

```
Source Files (.rs, .py, .ts, .tsx, .js, .jsx)
    │
    ▼
┌─────────────────┐
│  LanguageHandler │  Trait-based: RustHandler, PythonHandler, TypeScriptHandler
│  (OnceLock reg.) │  Grammar + query patterns + docstring + call extraction per language
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│   CodeAnalyzer  │  tree-sitter AST → function/class chunks with docstrings + calls
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│   Reconciler    │  SHA256 hash comparison: skip unchanged, nuke+replace changed
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│   Orchestrator  │  Async I/O: embed new chunks, delete stale, insert fresh
└────────┬────────┘
         │
         ▼
    LanceDB (4 tables)
```

## Docstring Extraction

| Language | Strategy | Patterns |
|----------|----------|----------|
| Rust | Scan backwards from node | `///` outer doc, `#[doc = "..."]` attribute form |
| Python | AST traversal into body | `"""..."""` / `'''...'''` first expression_statement |
| TypeScript | Scan backwards for JSDoc | `/** ... */`, filters out `@param`/`@returns` |

## Key Design Decisions

1. **Function-level chunking**: 1 function/class → 1 vector for precise retrieval
2. **4-table schema**: Separate tables for different content types with specialized formatting
3. **Trait-based language abstraction**: `LanguageHandler` trait — add new languages by implementing 5 methods
4. **Incremental ingestion**: Three-layer architecture (Parse→Reconcile→Orchestrate) with SHA256 file hashing
5. **Docstrings in embeddings and context**: Extracted docs enrich both semantic search and LLM prompt
6. **Call enrichment in embeddings**: `Calls: foo, bar` appended to embedding text — probabilistic relationship signal
7. **Intent classification via prototype embeddings**: Cosine similarity against pre-embedded prototype queries, not keyword heuristics
8. **Declarative routing table**: `HashMap<QueryIntent, RetrievalConfig>` — data, not code. New intents = new entries
9. **Scored-only search API**: `search_*()` returns `Vec<(T, f32)>` — distance always available, single code path
10. **Distance → relevance**: `1.0 / (1.0 + dist)` — simple, monotonic, metric-agnostic
11. **Two-consumer SoC**: Context builder uses chunk content (ignores scores). Source builder uses scores (ignores content)
12. **Mutex on Embedder**: Only resource needing synchronization (model weights)
13. **Leptos WASM frontend**: Client-side rendered SPA with reactive signals
14. **Two-stage Docker**: Separate ingestion from query serving
15. **Shared engine crate**: `code-rag-engine` contains pure algorithms — compiles to both native and wasm32
16. **Feature-flag deployment**: `code-rag-ui --features standalone` switches data source from backend API to in-browser RAG pipeline
17. **Closure-based decoupling**: `IntentClassifier::build()` takes embedding closure, not concrete type — works with fastembed (native) or tract-onnx (WASM)
18. **Optional LLM generation**: Retrieval pipeline works without auth; LLM answers are an add-on
19. **Quality harness with dual-run**: Measures recall@K, MRR, intent accuracy, latency across 43 test cases. Dual-run (classifier vs. ground-truth intent) isolates retrieval vs. classification quality
20. **Report metadata for parallel tracks**: `label` + `completed_tracks` in JSON reports enables comparison across independently-developed Track improvements

## Quality Harness (V3)

### Structural Foundation

V3 required a structural refactor: module declarations moved from `main.rs` to `src/lib.rs`, enabling a second binary target (`code-rag-harness`) to share library code. `FlatChunk` + `RetrievalResult::flatten()` centralize chunk flattening — used by both API (`build_sources()`) and harness evaluation. Single modification point when new chunk types are added.

### Test Dataset (V3.1)

43-query declarative test corpus with typed expectations. Three-tier strategy:

| Tier | Count | Expectations | Purpose |
|------|-------|-------------|---------|
| Hero | 5 | All dimensions (files, identifiers, chunk types, projects, intent) | Regression anchors |
| Directional | 20+ | 1-2 dimensions per intent category | Track quality per intent |
| Smoke | 7 | Only `min_relevant_results` / `excluded_files` | Pipeline-agnostic sanity |

Forward-compatible schema: all fields `Option<T>` or `Vec<T>` with `#[serde(default)]`. Future Track fields can be added without breaking existing cases.

### Harness Binary (V3.2)

Second binary (`code-rag-harness`) measures retrieval quality by running test queries against the real engine pipeline, stopping before LLM generation.

```
data/test_queries.json (43 cases)
    │
    ▼
┌─────────────────┐
│     Runner      │  embed → classify → route → retrieve (per query)
└────────┬────────┘
         │
    ┌────┴────┐
    ▼         ▼
┌────────┐ ┌────────┐
│Matching│ │Metrics │  recall@K, MRR, intent accuracy, latency
└────┬───┘ └────┬───┘
     │          │
     ▼          ▼
┌─────────────────┐
│     Report      │  JSON + Markdown, per-intent breakdown, warnings
└─────────────────┘
```

Matching: substring for file paths (survives directory restructuring), exact for identifiers/projects/chunk types. Recall excludes coverage checks — `expected_chunk_types`, `expected_projects`, `min_relevant_results`, and `excluded_files` are boolean checks alongside recall.

### Baseline (V3.3)

**Dual-run mode:** Full pipeline (real classifier) vs. ground-truth intent (bypassed classifier) isolates classifier-induced recall loss.

| Metric | Full Pipeline | Ground-Truth |
|--------|--------------|-------------|
| recall@5 | 0.65 | 0.67 |
| MRR | 0.60 | 0.61 |
| Intent accuracy | 62% | 100% |

Per-intent recall@5: overview 1.00, implementation 0.70, comparison 0.75, relationship 0.38. Ground-truth routing improves recall by only +0.02 — retrieval quality is the bottleneck, not classification. Report metadata (`label`, `completed_tracks`) enables comparison across parallel Track improvements.

## Intent-Aware Retrieval

| Intent | code | readme | crate | module_doc |
|--------|------|--------|-------|------------|
| Overview | 5 | 3 | 3 | 3 |
| Implementation | 5 | 1 | 1 | 2 |
| Relationship | 5 | 1 | 2 | 2 |
| Comparison | 5 | 2 | 3 | 2 |

## Build & Run

```bash
# Ingest repositories
docker-compose -f docker-compose-ingest.yaml up

# Run query server (Docker)
docker-compose up

# Export data for static demo
cargo run -p code-raptor -- export --db-path data/portfolio.lance --output crates/code-rag-ui/static/index.json

# Build static GitHub Pages demo
trunk build --release --features standalone crates/code-rag-ui/index.html

# Run quality harness (dual-run baseline)
cargo run --release --bin code-rag-harness -- --verbose
cargo run --release --bin code-rag-harness -- --ground-truth-intent --label baseline_gt --verbose
```
