# Architecture report: code-rag

Emergent topology over 775 nodes and 2355 relation edges, partitioned into 39 communities.

## Read these first

The most-connected definitions — understanding these unlocks the most of the codebase.

| Definition | File | Degree | Community |
|---|---|---|---|
| `new` | code-rag/crates/code-rag-store/src/vector_store.rs | 39 | 2 |
| `VectorStore` | code-rag/crates/code-rag-store/src/vector_store.rs | 37 | 2 |
| `retrieve` | code-rag/crates/code-rag-core/src/retriever.rs | 30 | 0 |
| `run_retrieval` | code-rag/crates/code-rag-ui/src/standalone_api.rs | 24 | 0 |
| `ScoredChunk` | code-rag/crates/code-rag-engine/src/retriever.rs | 21 | 0 |
| `run_ingestion` | code-rag/crates/code-rag-ingest/src/ingestion/mod.rs | 19 | 8 |
| `Embedder` | code-rag/crates/code-rag-store/src/seams.rs | 19 | 3 |
| `content_hash` | code-rag/crates/code-rag-types/src/lib.rs | 18 | 8 |
| `main` | code-rag/src/bin/harness.rs | 18 | 1 |
| `Err` | code-rag/crates/code-rag-engine/src/intent.rs | 18 | 26 |

## Communities

| Id | Size | Cohesion | Likely concern | Directory |
|---|---|---|---|---|
| 0 | 70 | 0.08 | `retrieve` | code-rag/crates/code-rag-ui/src |
| 1 | 57 | 0.07 | `main` | code-rag/src/harness |
| 2 | 46 | 0.12 | `new` | code-rag/crates/code-rag-store/src |
| 3 | 40 | 0.13 | `Embedder` | code-rag/crates/code-rag-store/src |
| 4 | 38 | 0.06 | `chat` | code-rag/src/api |
| 5 | 31 | 0.11 | `graph_augment` | code-rag/crates/code-rag-engine/src |
| 6 | 28 | 0.09 | `ChatView` | code-rag/crates/code-rag-ui/src |
| 7 | 26 | 0.11 | `get_table` | code-rag/crates/code-rag-store/src |
| 8 | 24 | 0.21 | `run_ingestion` | code-rag/crates/code-rag-ingest/src/ingestion |
| 9 | 22 | 0.25 | `run_export` | code-rag/crates/code-rag-ingest/src |
| 10 | 20 | 0.10 | `FolderChunk` | code-rag/crates/code-rag-types/src |
| 11 | 19 | 0.12 | `batches_to_crate_chunks_hybrid` | code-rag/crates/code-rag-store/src |
| 12 | 18 | 0.12 | `extract_comparators` | code-rag/crates/code-rag-engine/src |
| 13 | 18 | 0.21 | `RerankText` | code-rag/crates/code-rag-engine/src |
| 14 | 17 | 0.21 | `GoHandler` | code-rag/crates/code-rag-ingest/src/ingestion/languages |
| 15 | 17 | 0.20 | `ingest_repo` | code-rag/crates/code-rag-ingest/src |
| 16 | 17 | 0.18 | `run_search` | code-rag/crates/code-rag-mcp/src |
| 17 | 17 | 0.14 | `new` | code-rag/crates/code-rag-ingest/src/ingestion |
| 18 | 17 | 0.23 | `LanguageHandler` | code-rag/crates/code-rag-ingest/src/ingestion |
| 19 | 16 | 0.18 | `TypeScriptHandler` | code-rag/crates/code-rag-ingest/src/ingestion/languages |
| 20 | 14 | 0.24 | `RustHandler` | code-rag/crates/code-rag-ingest/src/ingestion/languages |
| 21 | 13 | 0.24 | `collect_type_idents` | code-rag/crates/code-rag-ingest/src/ingestion/languages |
| 22 | 13 | 0.15 | `get_chunks_by_ids` | code-rag/crates/code-rag-store/src |
| 23 | 12 | 0.30 | `CallEdge` | code-rag/crates/code-rag-store/src |
| 24 | 11 | 0.29 | `new` | code-rag/crates/code-rag-store/src |
| 25 | 11 | 0.26 | `FileMeta` | code-rag/crates/code-rag-engine/src |
| 26 | 10 | 0.24 | `Err` | code-rag/crates/code-rag-mcp/src |
| 27 | 9 | 0.18 | `build_sources` | code-rag/src/api |
| 28 | 8 | 0.32 | `new` | code-rag/crates/code-rag-mcp/src |
| 29 | 8 | 0.36 | `generate` | code-rag/crates/code-rag-ui/src |
| 30 | 8 | 0.42 | `build_file_chunks` | code-rag/crates/code-rag-ingest/src/ingestion |
| 31 | 6 | 0.33 | `batches_to_module_doc_chunks` | code-rag/crates/code-rag-store/src |
| 32 | 6 | 0.48 | `build_folder_chunks` | code-rag/crates/code-rag-ingest/src/ingestion |
| 33 | 6 | 0.53 | `extract_docstring` | code-rag/crates/code-rag-ingest/src/ingestion/languages |
| 34 | 5 | 0.40 | `batches_to_folder_chunks_hybrid` | code-rag/crates/code-rag-store/src |
| 35 | 4 | 0.50 | `code_rag_neighbors` | code-rag/crates/code-rag-mcp/src |
| 36 | 3 | 0.83 | `TopologyOpts` | code-rag/crates/code-raptor/src |
| 37 | 1 | 1.00 | `loadPipeline` | code-rag/crates/code-rag-ui/static |
| 38 | 1 | 1.00 | `loadReranker` | code-rag/crates/code-rag-ui/static |

## Cross-module bridges

Edges carrying the most shortest-path traffic *between* communities — hidden coupling.

| From | To | Betweenness | Weight | Communities | Pair edges |
|---|---|---|---|---|---|
| `collect_type_idents` (code-rag/crates/code-rag-ingest/src/ingestion/language.rs) | `String` (caravan/internal/compiler/diag.go) | 8575.0 | 1 | 0 ↔ 21 | 6 |
| `ErrorBody` (code-rag/src/api/error.rs) | `String` (caravan/internal/compiler/diag.go) | 7760.0 | 1 | 0 ↔ 4 | 16 |
| `retrieve` (code-rag/crates/code-rag-core/src/retriever.rs) | `Result` (quant-trading-gym/crates/sim-core/src/error.rs) | 6051.2 | 1 | 0 ↔ 2 | 25 |
| `CallEdge` (code-rag/crates/code-rag-types/src/lib.rs) | `String` (caravan/internal/compiler/diag.go) | 4359.8 | 1 | 0 ↔ 23 | 2 |
| `FlatChunk` (code-rag/crates/code-rag-engine/src/retriever.rs) | `String` (caravan/internal/compiler/diag.go) | 4219.2 | 2 | 0 ↔ 1 | 38 |
| `main.rs` (code-rag/crates/code-rag-mcp/src/main.rs) | `Result` (quant-trading-gym/crates/sim-core/src/error.rs) | 3895.8 | 1 | 2 ↔ 16 | 6 |
| `config.rs` (code-rag/crates/code-rag-engine/src/config.rs) | `fetch_limits` (code-rag/crates/code-rag-engine/src/config.rs) | 3679.8 | 1 | 0 ↔ 1 | 38 |
| `Result` (quant-trading-gym/crates/sim-core/src/error.rs) | `classify` (code-rag/crates/code-rag-core/src/intent_local.rs) | 3567.2 | 1 | 2 ↔ 4 | 6 |
| `new` (code-rag/crates/code-rag-mcp/src/main.rs) | `String` (caravan/internal/compiler/diag.go) | 3349.2 | 1 | 0 ↔ 28 | 4 |
| `rerank` (code-rag/crates/code-rag-store/src/reranker.rs) | `String` (caravan/internal/compiler/diag.go) | 3150.4 | 1 | 0 ↔ 24 | 4 |

## Surprising connections

Bridges re-ranked by unexpectedness (betweenness × weight / links between the pair): one of only a few edges tying two otherwise-separate communities together.

| From | To | Surprise | Weight | Communities | Pair edges |
|---|---|---|---|---|---|
| `TopologyOpts` (code-rag/crates/code-raptor/src/lib.rs) | `String` (caravan/internal/compiler/diag.go) | 3234.2 | 2 | 0 ↔ 36 | 1 |
| `lib.rs` (code-rag/crates/code-rag-types/src/lib.rs) | `CallEdge` (code-rag/crates/code-rag-types/src/lib.rs) | 2795.7 | 1 | 10 ↔ 23 | 1 |
| `build` (code-rag/crates/code-rag-engine/src/intent.rs) | `Result` (quant-trading-gym/crates/sim-core/src/error.rs) | 2707.7 | 2 | 2 ↔ 12 | 1 |
| `Result` (quant-trading-gym/crates/sim-core/src/error.rs) | `main` (code-rag/src/main.rs) | 2319.7 | 1 | 2 ↔ 27 | 1 |
| `CallEdge` (code-rag/crates/code-rag-types/src/lib.rs) | `String` (caravan/internal/compiler/diag.go) | 2179.9 | 1 | 0 ↔ 23 | 2 |
| `generate` (code-rag/crates/code-rag-ui/src/gemini.rs) | `Result` (quant-trading-gym/crates/sim-core/src/error.rs) | 2144.9 | 1 | 2 ↔ 29 | 1 |
| `Error` (caravan/internal/compiler/diag.go) | `ChatView` (code-rag/crates/code-rag-ui/src/components/chat_view.rs) | 1684.2 | 1 | 6 ↔ 11 | 1 |
| `collect_type_idents` (code-rag/crates/code-rag-ingest/src/ingestion/language.rs) | `String` (caravan/internal/compiler/diag.go) | 1429.2 | 1 | 0 ↔ 21 | 6 |
| `Error` (caravan/internal/compiler/diag.go) | `seams.rs` (code-rag/crates/code-rag-store/src/seams.rs) | 1303.0 | 1 | 4 ↔ 11 | 1 |
| `build_topology` (code-rag/crates/code-raptor/src/lib.rs) | `Result` (quant-trading-gym/crates/sim-core/src/error.rs) | 1210.4 | 1 | 2 ↔ 36 | 1 |

## Dependency cycles

- code-rag/crates/code-rag-ingest/src/ingestion/mod.rs → code-rag/crates/code-rag-ingest/src/ingestion/reconcile.rs → code-rag/crates/code-rag-ingest/src/ingestion/mod.rs

## Questions this topology can answer

- What does `new` do, and what depends on it?
- What is the subsystem around `retrieve` (community 0) responsible for?
- Why are `TopologyOpts` and `String` coupled across module boundaries?
- What would it take to break the circular dependency code-rag/crates/code-rag-ingest/src/ingestion/mod.rs → code-rag/crates/code-rag-ingest/src/ingestion/reconcile.rs?
