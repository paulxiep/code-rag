# Portfolio RAG Chat — Executive Summary

## What It Is

A RAG (Retrieval-Augmented Generation) chatbot that answers questions about code repositories. Parses Rust, Python, TypeScript, and Go codebases with tree-sitter, extracts docstrings and call graphs, generates embeddings with FastEmbed, stores in LanceDB, and responds via Google Gemini. Intent classification routes queries to optimized retrieval strategies, and retrieval traces surface all sources with relevance scores.

## Why It Matters

- **Portfolio showcase**: Demonstrates Rust, RAG architecture, and chatbot development skills
- **Meta-project**: Can answer questions about itself and other portfolio projects
- **Code understanding**: Semantic search over function-level code chunks
- **Releasable dev tool (MCP server)**: The retrieval brain ships as `code-rag-mcp`, a single-binary Model Context Protocol server any developer can install in three steps (download exe → edit one YAML → run exe) to give Claude Code intent-routed retrieval, call-graph traversal, architecture overviews, and topology insight — emergent-module maps with folder-drift comparison, dependency-cycle detection, and call-path tracing — no terminal commands, no API keys.

## Key Features

- **Multi-language parsing**: Rust, Python, TypeScript, and Go via tree-sitter AST queries
- **Docstring extraction**: `///` (Rust), `"""` (Python), `/** */` (TypeScript JSDoc), `//` (Go) — enriches embeddings and LLM context
- **Hierarchy chunks (Track A)**: `FolderChunk` (1 per directory; 5-line template — folder/files+languages/key types/key functions/subfolders, ~118 in portfolio) and `FileChunk` (1 per source file; 4-line template — file/exports/imports/purpose, ~247 in portfolio). Built deterministically at ingest from CodeChunk metadata + C1 imports map — no LLM. Pure render functions in `code-rag-engine::{folder,file}` keep server-embedded bytes byte-identical to browser BM25 bytes
- **Single text module (A1)**: `code-rag-engine::text` is the sole home for `tokenize`, `IdfTable`, BM25 kernel, `build_searchable_text`, `split_camel_case`, and intent prototype texts — compiles to native + wasm32. No more drift between server / store / raptor / UI
- **Persistent call graph (Graph RAG)**: LanceDB scalar-only `call_edges` table, 3-tier resolver (same-file → import-based → unique-global), graph traversal (callers/callees/path) augments retrieval at query time. AST scoped-identifier (`module::function()`) extraction added
- **Typed relation topology (Track R)**: `graph_edges` persists imports / re_exports / contains / implements / extends / embeds / references / rationale_for edges, resolved per-language with anchored rules and a project-scoped identifier index — the graph never links sibling projects while retrieval stays corpus-wide
- **Emergent communities (Code Raptor)**: deterministic Louvain (fixed iteration order, no RNG — identical partition and community ids on every run) over the relation topology, with utility-hub exclusion, oversized/low-cohesion re-splits, and per-community cohesion scores persisted in `community_assignments`. Folder→file containment is excluded from the partition input so the emergent structure can be honestly compared against the folder layout
- **Architecture report + drift**: one byte-deterministic markdown artifact per project — "read these first" centrality ranking, cross-community bridges with relation provenance, surprising-connection ranking, import-cycle detection (Tarjan SCC + bounded canonical DFS), and an emergent-vs-folder **drift** section (community purity vs directory layout)
- **Interactive topology view**: demo tab rendering each project's community graph with d3-force (community-colored, degree-sized, relation-dashed, theme-aware); clicking a node auto-submits a code-rag query about it
- **Topology exports**: per-project viz JSON (browser artifact, capped) + full-graph GraphML (opens in Gephi/yEd), byte-identical across runs
- **Test code exclusion at ingest**: 3-level — directory `tests/`, filename `test_*.py` / `*.test.ts`, AST-walked `#[cfg(test)]` enclosing-mod detection. Removed ~24% of chunks (3772 → 2861)
- **Intent classification**: Cosine similarity against prototype query embeddings + k-NN (k=3) weighted voting + keyword pre-filter with adversarial guards — 74% accuracy (semantic, not keyword-based)
- **Query routing**: Declarative routing table maps intent (overview, implementation, relationship, comparison) to per-type retrieval limits across all six chunk types (code, folder, file, readme, crate, module_doc)
- **Two-stage retrieval**: Hybrid BM25 (on `searchable_text`) + dense vector search fused with RRF → cross-encoder reranking (`ms-marco-MiniLM-L-6-v2`, ONNX)
- **Per-intent `ArmPolicy`**: `{body_vec, sig_vec, bm25, rerank, folder_vec, file_vec}` gates per intent — single source of truth shared server + browser. Folder arm gated off for Relationship after empirical regression (folder chunks of X displaced consumers of X — stratified retrieval pattern: file arm stays on for Relationship import-graph queries)
- **Graph result protection**: SOTA routing partitions graph hits **out** of the reranker entirely for explicit-direction queries ("what calls X / called by"); soft-reserve over-retains the code arm by `+5` and rescues demoted graph chunks for ambiguous-direction. Identical logic in server (`src/engine/retriever.rs`) and WASM standalone (`crates/code-rag-ui/src/standalone_api.rs`)
- **Comparison query decomposition**: Regex extracts ≥2 comparators → per-comparator body-vec sub-searches (comparator name prepended to original query) → vote-based dominant-project filter → RRF fusion → max-of-natural score rescoring (so RRF outputs compete on equal terms with distance-converted non-code arms). Pure-Rust, wasm32-compatible (`code-rag-engine::comparison`)
- **Declaration signatures**: Functions + structs/enums/traits/interfaces/classes extracted at ingest; power `searchable_text` (identifier 2× boost + camelCase split + signature + docstring) as the FTS target
- **Dual-vector schema**: Nullable `signature_vector` column (shipped OFF after empirical space sweep; column retained)
- **Retrieval traces**: All 6 chunk types surfaced with relevance scores, sorted by relevance — the system shows its work
- **Quality harness**: 87-query test dataset (79 recall-scoreable, +48 B4 held-out classifier cases) with automated recall@K, **`recall@pool`** (introduced in A4 — recall over every chunk reaching `build_context`, no top-k truncation), MRR, intent accuracy, and latency measurement — dual-run mode isolates classifier vs. retrieval quality
- **Multi-binary crate**: `src/lib.rs` extraction enables `code-rag-harness` second binary alongside the main server — shared library, independent entry points
- **Centralized chunk flattening**: `FlatChunk` + `flatten()` — single source of truth for both API responses and harness evaluation
- **Incremental ingestion**: SHA256 file hashing skips unchanged files for fast re-indexing
- **6 chunk types**: Code functions, **Folder summaries**, **File summaries**, README files, Crate metadata, Module docs
- **Trait-based language abstraction**: Add new languages by implementing `LanguageHandler` trait
- **Vector search**: LanceDB with FastEmbed (BGE-small-en-v1.5, 384 dimensions)
- **LLM integration**: Google Gemini via rig-core (optional — retrieval works without API key)
- **Web UI**: Leptos WASM SPA (Rust compiled to WebAssembly)
- **GitHub Pages demo**: Full RAG pipeline runs in-browser via `standalone` feature — no backend needed
- **Shared engine**: `code-rag-engine` crate compiles to both native and wasm32
- **Claude Code MCP server**: Nine tools — retrieval (`code_rag_search`, `code_rag_overview`, `code_rag_graph`, `code_rag_neighbors`, `code_rag_reindex`) plus topology insight (`code_rag_communities` with drift comparison, `code_rag_central_nodes`, `code_rag_cycles`, `code_rag_path` with Mermaid call-flow output) — and a bundled Claude Code skill that routes queries between built-in Grep/Read and the MCP tools. Single-binary install: download `code-rag-mcp` from the GitHub Release, edit a YAML config (`target_path` + `workspace: true|false`), run the exe — it writes the skill + `.mcp.json` + `.gitignore` entry into the target dir, then exits. The first ingest happens transparently when the agent makes its first MCP call. Manually-triggered release workflow ships matrix-built binaries for Linux / macOS / Windows; the embedded `ingest` subcommand subsumes the standalone index-builder so end users only need one binary.

## Quick Start

```bash
# 1. Ingest repositories
docker-compose -f docker-compose-ingest.yaml up

# 2. Run query server
docker-compose up
```

Open http://localhost:3000 for the chat interface.

## Current Quality Metrics (2026-08, 6-project corpus)

Measured with the composite per-intent `ArmPolicy`, hybrid + rerank, classifier routing (label `post_rationale_anchor`, commit 8064e31):

| Metric | Classifier |
|--------|:---------:|
| recall@5 (aggregate) | 0.60 |
| recall@10 (aggregate) | 0.69 |
| **recall@pool** (aggregate) | **0.72** |
| Intent accuracy (97-case held-out corpus) | 74% |

Per-intent recall@5 / @10 / @pool: overview 0.70/0.77/0.81, implementation 0.61/0.67/0.67, relationship 0.47/0.61/0.61, comparison 0.62/0.73/0.75.

**Corpus note — not comparable to the earlier post-A4 0.72@5**: two portfolio projects were purged (source repos deleted), the cross-project resolution leak was fixed (edges that once resolved into other projects inflated recall with wrong answers), and the corpus grew to include code-rag's own Track R code. The drop is a measurement-context change, not a quality regression — the leak fix itself moved per-intent numbers only within noise (comparison identical, others ±2–3pp), and the rest tracks months of corpus drift against a frozen test dataset.

The lifts came from the retrieval infrastructure: cross-encoder reranking (B1), hybrid BM25+dense with RRF (B2), signature-aware `searchable_text` (B3), and a tuned per-intent `ArmPolicy` (B5). B4 raised classifier accuracy 58%→74%, closing the classifier-vs-GT retrieval gap to ~2pp — classification is no longer the dominant bottleneck. Track C added Graph RAG (C1), graph result protection (C2), and comparison query decomposition (C3). Track A consolidated text primitives into one wasm-pure module (A1) and added the folder/file hierarchy arms (A2–A4; `recall@pool` introduced as the more faithful RAG-pipeline metric). Track R is retrieval-neutral by design — its one retrieval experiment (R3 cluster-summary arm) was measured, found net-negative, and gated OFF per the project's empirical-gating standard, with the machinery left wired for a revisit.

## Current State

547 tests, 0 warnings:
- `code-rag-ingest`: Ingestion CLI — trait-based language handlers (Rust, Python, TypeScript, Go), incremental ingestion, docstring + signature + call extraction, **3-tier call edge resolution** (project-scoped identifier index), **typed relation-edge extraction** (imports/type relations/containment/rationale), **3-level test code exclusion**, **folder + file chunk builders**, data export (incl. IDF tables + call edges + folder/file/cluster IDFs)
- `code-raptor`: **Topology engine (Track R)** — builds the relation topology from persisted edges, deterministic Louvain community detection + cohesion, ClusterChunk summaries, structural analytics (centrality / betweenness bridges / surprise ranking / import cycles / **emergent-vs-folder drift**), byte-deterministic architecture report + viz JSON + GraphML writers, `insights` facade backing the MCP topology tools
- `code-rag-engine`: Shared algorithms — intent classification (k-NN + pre-filter + comparator extraction), context building, scoring, N-ary `rrf_fuse`, `ArmPolicy`, **`graph` (`graph_augment`, `path_augment`, `merge_graph_chunks`, `reserve_graph_slots`, `detect_direction`, `RelationGraph`)**, **`centrality` (degree ranking, browser-identical)**, **`mermaid` (call-path flowcharts)**, **`comparison`**, **`text` (single-source tokenize/IDF/BM25/searchable_text — A1)**, **`folder` / `file` / `cluster` summary templates** (compiles to native + wasm32)
- `code-rag-store`: Embedder + VectorStore + Reranker — scored search API, hybrid BM25+vector search via LanceDB FTS, 7 vector tables + 3 scalar tables (`call_edges`, `graph_edges`, `community_assignments`), `VectorReader`/`VectorWriter` Caravan seams incl. topology reads
- `code-rag-types`: Shared types — deterministic chunk/edge IDs, content hashes, **`CallEdge` + `GraphEdge` (typed relations) + `CommunityAssignment` + `ClusterChunk`**, **`FolderChunk` + `FileChunk`**
- `code-rag-core` / `code-rag-llm`: chat-side core (`AppState`, `retrieve`) and LLM-provider impls, extracted at Caravan M5
- `code-rag-chat`: Query API — retrieval (graph augmentation + comparison decomposition pre-branch + folder/file arms), LLM, quality harness (with **`recall@pool` metric**), serves WASM UI
- `code-rag-mcp`: MCP stdio server — nine tools (five retrieval + four topology), bundled Claude Code skill, single-binary release
- `code-rag-ui`: Leptos WASM SPA — chat + **topology tabs**; standalone mode runs the full RAG pipeline in-browser and renders each project's community graph (d3-force, click-a-node → query); back-compat with older `index.json` bundles via `#[serde(default)]`

## Technology

- **Language**: Rust
- **Web Framework**: Axum 0.8
- **LLM**: Google Gemini (rig-core 0.27)
- **Vector Database**: LanceDB
- **Embeddings**: FastEmbed (BGE-small-en-v1.5)
- **Code Parsing**: tree-sitter (Rust, Python, TypeScript/TSX, Go)
- **Frontend**: Leptos 0.8 (Rust WASM, CSR)
- **Deployment**: Docker (local) + GitHub Pages (static demo)
