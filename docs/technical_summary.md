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
| `code-raptor` | Ingestion CLI — tree-sitter parsing, language handlers, incremental ingestion, docstring + call extraction | `ingestion/parser.rs`, `ingestion/language.rs`, `ingestion/languages/`, `main.rs` |
| `code-rag-store` | Embedder (FastEmbed) + VectorStore (LanceDB) with scored search API | `embedder.rs`, `vector_store.rs` |
| `code-rag-types` | Shared types — CodeChunk, ReadmeChunk, etc. with UUID, content_hash | `lib.rs` |
| `code-rag-chat` | Query API — intent classification, query routing, retrieval traces, context builder, LLM | `api/`, `engine/` |

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
13. **htmx frontend**: Server-rendered HTML with async updates, minimal JS
14. **Two-stage Docker**: Separate ingestion from query serving

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

# Run query server
docker-compose up

# Clean up
sh clean_docker.sh
```
