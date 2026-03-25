# Portfolio RAG Chat — Technical Summary

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    Cargo Workspace                          │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  ┌─────────────────┐   ┌─────────────────┐                  │
│  │   code-raptor   │   │ portfolio-rag-  │                  │
│  │   (Indexing)    │   │     chat        │                  │
│  │                 │   │  (Query API)    │                  │
│  │  - CLI          │   │                 │                  │
│  │  - tree-sitter  │   │  - Axum server  │                  │
│  │  - walkdir      │   │  - LLM client   │                  │
│  └────────┬────────┘   └────────┬────────┘                  │
│           │                     │                           │
│           ▼                     ▼                           │
│  ┌─────────────────────────────────────────┐                │
│  │           code-rag-store                  │                │
│  │  - Embedder (FastEmbed)                 │                │
│  │  - VectorStore (LanceDB)                │                │
│  └─────────────────┬───────────────────────┘                │
│                    │                                        │
│                    ▼                                        │
│  ┌─────────────────────────────────────────┐                │
│  │           code-rag-types                  │                │
│  │  - CodeChunk, ReadmeChunk               │                │
│  │  - CrateChunk, ModuleDocChunk           │                │
│  └─────────────────────────────────────────┘                │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

## Crate Responsibilities

| Crate | Purpose | Key Files |
|-------|---------|-----------|
| `code-raptor` | Ingestion CLI — tree-sitter parsing, language handlers, incremental ingestion, data export | `ingestion/`, `export.rs`, `main.rs` |
| `code-rag-engine` | Shared algorithms — intent classification, context building, scoring (pure, no I/O, compiles to wasm32) | `intent.rs`, `context.rs`, `config.rs`, `retriever.rs` |
| `code-rag-store` | Embedder (FastEmbed) + VectorStore (LanceDB) with scored search API | `embedder.rs`, `vector_store.rs` |
| `code-rag-types` | Shared types — CodeChunk, ReadmeChunk, etc. with UUID, content_hash | `lib.rs` |
| `code-rag-chat` | Query API — retrieval, LLM, serves WASM UI | `api/`, `engine/` |
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
```
