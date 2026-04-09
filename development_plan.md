# CodeRAG: Vertical Development Plan

Ideated with LLM assistance, structured for agile-friendly milestones.

Refer to [project vision](project-vision.md) for full improvement ideas and [architecture](architecture.md) for technical design.

---

## Philosophy

**Build vertically, not horizontally.**

Each iteration delivers a thin slice through the workspace crates (code-raptor, code-rag-chat, code-rag-engine, code-rag-ui). Every version produces something *runnable* and *demonstrable*.

**Value proposition: Decouple knowledge from reasoning.**

Rich, structured retrieval amplifies *any* model—cheap or frontier. By offloading "what context is relevant" to the retrieval layer, the model's complexity budget can be spent on reasoning, multi-step workflows, or tool orchestration. This scales independently of model choice: better retrieval benefits Haiku and Opus alike.

---

## Guiding Mantra

> **"Declarative, Modular, SoC"**

Every implementation decision should be evaluated against these three principles:

| Principle | Meaning | Example |
|-----------|---------|---------|
| **Declarative** | Describe *what*, not *how*. Config over code. Data-driven behavior. | Chunk types declare their schema; retrieval strategies defined by config, not hardcoded. Intent routing rules are data, not if-else chains. |
| **Modular** | Components are self-contained, swappable, and independently testable. | code-raptor and code-rag-chat share no code, only LanceDB schema. code-rag-engine compiles to both native and wasm32. Swap HDBSCAN for hierarchical clustering without touching summarization. |
| **SoC** (Separation of Concerns) | Each module has ONE job. No god objects. Clear boundaries. | code-raptor = indexing. code-rag-chat = querying. code-rag-engine = algorithms. code-rag-ui = frontend. Types live in code-rag-types. No crate does two things. |

**Before writing code, ask:**
1. Am I describing behavior or implementing mechanics? (Declarative)
2. Can this be swapped out without ripple effects? (Modular)
3. Does this component have exactly one responsibility? (SoC)

---

## Additional Principles

| Principle | Meaning | Example |
|-----------|---------|---------|
| **Research vs Production** | Experimental features get dedicated versions with evaluation criteria | RAPTOR clustering is time-boxed research, not bundled with stable features |
| **Incremental Value** | Each version improves user-facing capability | V1 enables query routing, not just "indexing works" |

---

## Architecture Overview

```
code-raptor (producer)          code-rag-chat (consumer)
    │                                   │
    │ writes chunks                     │ reads chunks
    ▼                                   ▼
              [LanceDB Schema]
              - CodeChunk (function-level)
              - ReadmeChunk, CrateChunk, ModuleDocChunk
              - FolderChunk (A, future)
              - FileChunk (A, future)
              - ClusterChunk (R, future)
              - CallEdge (C1, future)

code-rag-engine (shared pure algorithms, no I/O)
    ▲ used by code-rag-chat (native)
    ▲ used by code-rag-ui (WASM, standalone mode)

code-rag-ui (Leptos WASM frontend)
    [default]    → HTTP API → code-rag-chat
    [standalone] → in-browser RAG (transformers.js + code-rag-engine)
```

---

## Dependency Graph

### code-raptor (Indexing) Dependencies

```
Fix docstring extraction
    └──► Docstring generation (Track D) [must extract before generate]
            └──► RAPTOR clustering (Track R) [clusters on generated summaries]

Inline call context
    └──► Same-file call edges
        └──► Cross-file call graph

Folder/File-level embeddings
    └──► Auto-generate repo summaries

Function signature extraction ─── independent (Track B)
Large function chunking ───────── independent (Track B)
Incremental ingestion ─────────── V1.3 (uses V1.1 schema, tightens project_name/paths)

LanguageHandler refactor ─────── V1.2 (pure refactor, unblocks V1.4 + V1.5)
    ├──► TypeScript support (V1.4)
    └──► Docstring extraction (V1.5, wires extract_docstring for all handlers)

RAPTOR clustering (Track R) ───── needs Track D enrichment + Track A hierarchy
    └──► Architecture comparison [requires Track A hierarchy]
```

### code-rag-chat + code-rag-engine (Query) Dependencies

```
Intent classification + query routing
    └──► Hierarchical query routing [requires folder/file chunks]

Cross-encoder reranking ────────── independent (Track B, query-side)

Hybrid search ─────────────────── independent (query-side)

Graph query interface ─────────── requires call graph data

Graph embeddings research ──────── requires C3 (call graph data + retrieval fixes)
```

### Parallelization Opportunities

| Can run in parallel | Rationale |
|---------------------|-----------|
| Track A + Track B + Track C + Track D | Independent after V3 |
| Track R after Track D | R clusters on D-generated summaries |
| A + B1 + C1 + D1 | All can start after V3 completes |
| V2.2 + V2.3 | All code-rag-chat, no dependencies |
| Folder/File embeddings + Hybrid search | Indexing vs query |

---

## Iteration Structure

```
V1 (Indexing Foundation) ─── incremental ingestion + docstrings     [COMPLETE]
 │
 ▼
V2 (Query Intelligence) ─── intent routing + retrieval traces       [COMPLETE]
 │  V2.1-V2.3: Backend intelligence
 │  V2.4: Leptos WASM frontend
 │  V2.5: GitHub Pages demo + code-rag-engine extraction
 │
 ▼
V3 (Quality Harness) ─── quantitative testing infrastructure
 │
 ├──► Track A: Semantic Understanding
 │       A: Hierarchy (folder/file embeddings + routing)
 │
 ├──► Track B: Search Precision
 │       B1 → B2 → B3 → B4 → B5
 │
 ├──► Track C: Relationship Graph
 │       C1 → C2 → C3
 │
 ├──► Track D: Enrichment Pipeline
 │       D1: Docstring Generation → D2: Type Inference
 │
 └──► Track R: RAPTOR Research (time-boxed, after Track D)
         R1 → R2 → R3 → R4 → R5
```

V1 → V2 → V3 are sequential. Tracks A, B, C, D can run in parallel after V3. Track R starts after Track D. Prioritize based on user needs.

### Effort Summary

| Phase | Effort | Cumulative |
|-------|--------|------------|
| **V1** (Indexing) | 2.5-3 weeks | 2.5-3 weeks |
| **V2** (Query) | 1 week | 2.5-3 weeks |
| **V3** (Testing) | 1 week | 3.5-4 weeks |
| **Track A** (A1→A5) | 1.5-2 weeks | — |
| **Track B** (B1→B2→B3→B4→B5) | 3-4 weeks | — |
| **Track C** (C1→C2→C3) | 3-5 weeks | — |
| **Track D** (D1→D2) | 1-1.5 weeks | — |
| **Track R** (R1→R5, after D) | 2 weeks | — |

**If running Tracks in parallel:** V1+V2+V3 (~4 weeks) + longest Track (~5 weeks) = **~9-10 weeks to full feature set**

**If running Tracks sequentially:** ~15-18 weeks total

### Track Priority (Portfolio Demo Context)

For portfolio demonstrations, hirers ask architecture questions first:

| Track | Priority | Rationale |
|-------|----------|-----------|
| **A (Hierarchy)** | High | "What does this folder do?" is a likely first question |
| **B1 (Reranking)** | High | Single highest-ROI change; 20-35% recall improvement |
| **B2 (Hybrid search)** | High | Precision for exact identifier queries |
| **D1 (Docstring Gen)** | Medium | Helps with undocumented code |
| **C1 (Same-file edges)** | Medium | Basic relationship queries |
| **R (RAPTOR)** | Research | Impressive if successful, time-boxed |

---

## V1: Indexing Foundation [COMPLETE]

**Goal:** Enable fast iteration, clean language abstraction, and fix docstring extraction. All code-raptor + code-rag-store work.

| Item | Status | Notes |
|------|--------|-------|
| V1.1 Schema Foundation | Done | UUID, content_hash, delete API, List deps, model version |
| V1.2 LanguageHandler Refactor | Done | Pure refactor: trait + registry, docstring stays None |
| V1.3 Incremental Ingestion | Done | File-level hashing, three-layer architecture, schema tightening |
| V1.4 TypeScript Support | Done | TypeScriptHandler with JSDoc extract_docstring |
| V1.5 Docstring Extraction | Done | Parser wiring, Rust + Python extraction, TypeScript activation |

### V1.1: Schema Foundation (FIRST)

**Why:** Current schema lacks fields needed for incremental operations and has design debt that compounds in later phases.

**Changes to code-rag-types:**
- Add `chunk_id: String` (UUID) - stable foreign key for Track C call graph edges
- Add `content_hash: String` - SHA256 of code_content for change detection
- Add `embedding_model_version: String` - prevents silent embedding inconsistency

**Changes to code-rag-store:**
- Add delete API: `delete_chunks_by_file()`, `delete_chunks_by_project()`
- Change `crate_chunks.dependencies` from CSV string to `List<Utf8>` - enables "what depends on X?" queries
- Update Arrow schemas for all 4 tables

**Crates:** code-rag-types, code-rag-store

### V1.2: LanguageHandler Refactor

**Why:** Current `SupportedLanguage` enum requires touching 4+ match statements per new language. Extract trait before adding languages or docstring extraction.

**Scope:** Pure refactor. `extract_docstring` defined on trait with default returning `None`. Ingestion output identical before and after. See `v1.2.md` for full design including all caller migration points.

**Key changes:**
- `LanguageHandler` trait with `name()`, `extensions()`, `grammar()`, `query_string()`, `extract_docstring()` (default None)
- `RustHandler`, `PythonHandler` implementations
- `handler_for_path()` registry replaces `SupportedLanguage::from_path()`
- Migrate all callers: `analyze_content()`, `extract_module_docs()`, `process_code_file()`
- Remove `SupportedLanguage` enum entirely

**Crate:** code-raptor

### V1.3: Incremental Ingestion

**Prerequisite:** V1.1 schema (UUID, content_hash, delete API)

**Architecture:** Three-layer (parse → reconcile → orchestrate). Parsing stays sync/testable, reconcile takes data only (no DB handle), main.rs orchestrates all async I/O.

**Comparison strategy:** File-level hashing. SHA256 of entire file content. Unchanged files are skipped entirely. Changed files: delete all old chunks, insert all new chunks. Simpler than per-chunk diffing with same performance characteristics.

**Schema tightening (absorbed into V1.3):**
- `project_name: Option<String>` → `String` on CodeChunk, CrateChunk, ModuleDocChunk
- Relative forward-slash path storage (portable across OS)
- CrateChunk content_hash includes description
- `--project-name` CLI flag for single-repo/multi-repo use

**Core incremental logic:**
- File-level hash comparison: skip unchanged, nuke+replace changed, delete orphaned
- CrateChunk comparison by `crate_name` (not file path)
- Deletions partitioned by LanceDB table (each chunk type in its own table)
- Batch delete API: `delete_chunks_by_ids()`
- Embedding model version check: detect mismatch, force `--full`
- `--full` flag for complete re-index, `--dry-run` for preview
- Insert-before-delete ordering (safer on crash)
- **Essential:** Enables fast iteration for all subsequent work
- **Crates:** code-rag-types, code-rag-store, code-raptor

### V1.4: TypeScript Support

**Prerequisite:** V1.2 LanguageHandler refactor

- Implement `TypeScriptHandler` with full trait (including `extract_docstring` for `/** */` JSDoc)
- Tree-sitter grammar integration (`tree-sitter-typescript`)
- File detection: `.ts`, `.tsx`, `.js`, `.jsx`
- Query patterns for: `function_declaration`, `arrow_function`, `method_definition`, `class_declaration`, `interface_declaration`, `type_alias_declaration`, `enum_declaration`
- Register in `languages/mod.rs` handler list
- Note: `extract_docstring` is implemented but remains unwired in parser.rs until V1.5
- **Crate:** code-raptor

### V1.5: Docstring Extraction [COMPLETE]

**Prerequisite:** V1.2 LanguageHandler refactor (docstring extraction is a trait method), V1.4 (TypeScriptHandler)

**Three concerns (SoC):**

1. **Wire parser.rs** — Extended `analyze_with_handler()` fold tuple to call `handler.extract_docstring(source, &node, source_bytes)` inside the fold closure where tree-sitter Nodes are still alive.

2. **Implement per-handler extraction:**
   - **RustHandler:** `///` outer doc comments (backward scan, aggregate lines), `#[doc = "..."]` attribute form, skip `#[derive]`/`#[cfg]` attributes between doc and item, preserve empty lines within doc blocks. `//!` (inner doc) scoped out — already handled by `extract_module_docs()`.
   - **PythonHandler:** AST traversal into body (`node → child_by_field_name("body") → first expression_statement → string`), `"""..."""` and `'''...'''` delimiters, PEP 257-style dedent for multi-line content.
   - **TypeScriptHandler:** `/** ... */` JSDoc (implemented in V1.4, activated by parser.rs wiring). Verified through pipeline with 5 dedicated tests.

3. **Context display** — `format_code_section()` in `context.rs` now includes `**Docs:**` line when docstring is present.

**Testing:** 97 tests pass (0 failures, 0 warnings). Unit tests per handler, cross-language pipeline tests in parser.rs, context display test.

**Crate:** code-raptor, code-rag-chat

**Deliverable:** Fast re-ingestion. Clean language abstraction. Docstrings in search results. TypeScript support with docstrings from day one. V1 milestone complete.

### V1 Hero Queries (Testing Checkpoint — Ready to Validate)
- "What is code-raptor?" → Explains ingestion pipeline with docstrings visible
- "How does the retriever work?" → Returns `retriever.rs` (self-reference verification)

---

## V2: Query Intelligence + Frontend [COMPLETE]

**Goal:** Make queries smarter with intent routing and visible retrieval sources. Deploy Leptos WASM frontend and GitHub Pages static demo.

| Item | Status | Notes |
|------|--------|-------|
| V2.1 Inline Call Context | Done | Ephemeral call extraction via tree-sitter, enriches embedding text (code-raptor) |
| V2.2 Intent Classification + Query Routing | Done | Cosine similarity classification, `QueryIntent` enum, `RoutingTable` HashMap (code-rag-engine) |
| V2.3 Retrieval Traces | Done | `ScoredChunk<T>`, all chunk types as sources, relevance scores (code-rag-engine, code-rag-store) |
| V2.4 Leptos Migration | Done | Leptos WASM SPA replaces htmx/Askama (code-rag-ui) |
| V2.5 GitHub Pages Demo | Done | code-rag-engine extraction, standalone feature, export subcommand |

### V2 Architecture Decisions

- **Calls are ephemeral**: `extract_calls()` returns `Vec<String>` alongside the parser fold tuple. Calls enrich embedding text via `format_code_for_embedding(id, lang, doc, code, calls)`, then are discarded. No `CodeChunk` struct change, no LanceDB schema change.
- **V2.2 classification + routing share `engine/intent.rs`**: Classification produces `QueryIntent`; routing maps it to `RetrievalConfig` via `RoutingTable`. Tightly coupled by design — one module, two functions.
- **Retriever stays intent-agnostic**: Handlers classify → route → pass `RetrievalConfig` to `retrieve()`. SoC preserved.
- **`ScoredChunk<T>` wrapper**: `RetrievalResult` wraps all chunks with relevance scores. Distance → relevance conversion happens once in the retriever.
- **Breaking `ChatResponse` API**: `SourceInfo` redesigned with `chunk_type`, `path`, `label`, `relevance`. All 4 chunk types surfaced, sorted by relevance. Acceptable pre-v1.0.

### V2.1: Inline Call Context

**Goal:** Append `Calls: foo, bar` to embedding text during code parsing, so functions that call other functions become semantically closer in vector space to queries about relationships and data flow. Lightweight precursor to Track C's persistent call graph.

**Architecture:** Ephemeral HashMap side-channel. `run_ingestion` returns `(IngestionResult, HashMap<String, Vec<String>>)`. The HashMap carries `chunk_id → call identifiers` from parser to embedder, completely bypassing `reconcile()`. Calls are consumed by `format_code_for_embedding(id, lang, doc, code, calls)`, baked into the embedding vector, then discarded. No `CodeChunk` struct change, no LanceDB schema change.

**Continuity with V1.5:** Same four-step extension pattern:

| Step | V1.5 (docstrings) | V2.1 (calls) |
|------|-------------------|--------------|
| Trait method | `extract_docstring() → Option<String>`, default `None` | `extract_calls() → Vec<String>`, default `Vec::new()` |
| Per-handler | Backward scan / AST / JSDoc | `call_expression` / `call` node walking |
| Fold extension | 4-tuple → 5-tuple | 5-tuple → 6-tuple |
| Downstream | Stored on `CodeChunk.docstring` | **Ephemeral side-channel** (diverges here) |

**SoC rationale for ephemeral design:** `code-rag-types` is the cross-crate data contract defining the LanceDB schema. Adding an ephemeral `calls` field would pollute the contract with embedding-pipeline-specific data. `CodeChunk` is used by reconcile, by the query-side retriever, and by context formatting — none need calls. Track C will store persistent call edges in a separate `call_edges` table.

**Per-language extraction:**
- **Rust:** `call_expression` → `identifier` (direct) or `field_expression > field_identifier` (method)
- **Python:** `call` → `identifier` (direct) or `attribute > identifier` (method)
- **TypeScript:** `call_expression` → `identifier` (direct) or `member_expression > property_identifier` (method)
- Each handler owns its AST semantics via private `collect_calls_recursive` helper

**Scope exclusions:** No macros (`macro_invocation` nodes, not `call_expression`), no variable-bound calls, no cross-file resolution (Track C scope).

**Breaking change:** `analyze_with_handler` return type changes from `Vec<CodeChunk>` to `Vec<(CodeChunk, Vec<String>)>`, requiring ~30 tests to add mechanical destructuring.

**Deployment:** Requires `code-raptor ingest <repo> --full` after deployment. `content_hash` is SHA256 of source file, not embedding text — incremental mode won't re-embed unchanged files.

**Crates affected:** code-raptor (`language.rs`, `languages/*.rs`, `parser.rs`, `mod.rs`, `main.rs`), code-rag-store (`embedder.rs`)

### V2.2: Intent Classification + Query Routing [COMPLETE]
- Embedding-based classification: cosine similarity against pre-computed prototype query embeddings
- `IntentClassifier` built at startup (~200ms), holds `HashMap<QueryIntent, Vec<Vec<f32>>>` prototypes
- `QueryIntent` enum: `Overview`, `Implementation`, `Relationship`, `Comparison`
- Default fallback: `Implementation` (most common code question type)
- `ClassificationResult` includes `confidence: f32` (cosine similarity score)
- Embed-once pipeline: query embedding reused for both classification and retrieval
- `retrieve()` takes `&[f32]` directly — no re-embedding, Mutex held ~5ms not ~50ms
- `RoutingTable`: `HashMap<QueryIntent, RetrievalConfig>` with default fallback
- code_limit fixed at 5 across all intents; differentiation in supplementary context only
- `EngineConfig.intent` removed; classifier lives in `AppState` as peer of `EngineConfig`
- **Crate:** code-rag-engine (classification + routing logic), code-rag-chat (AppState integration)

### V2.3: Retrieval Traces
- Extract `_distance` from LanceDB, convert to relevance: `1.0 / (1.0 + distance)`
- `ScoredChunk<T>` generic wrapper pairs each chunk with relevance score
- Redesigned `SourceInfo`: `chunk_type`, `path`, `label`, `project`, `relevance`, `line`
- All 4 chunk types surfaced as sources, sorted by relevance descending
- `ChatResponse` gains `intent` field
- **Demo value:** Makes retrieval quality visible; differentiator from black-box tools
- **Crate:** code-rag-chat, code-rag-store

**Deliverable:** Intent-based routing. Visible retrieval sources. Embeddings include call context.

### V2 Hero Queries (Testing Checkpoint)
- "How does the chat endpoint work?" → intent: implementation, sources include handlers.rs with relevance %
- "What is code-raptor?" → intent: overview, sources show README + CrateChunks ranking higher
- Overview vs implementation queries produce visibly different source distributions

### V2.4: Leptos Migration [COMPLETE]

**Goal:** Replace server-rendered htmx/Askama frontend with Leptos WASM SPA. Foundation for GitHub Pages static demo.

- New crate: `code-rag-ui` (Leptos 0.8 CSR, trunk build)
- Components: `ChatView`, `SourcesPanel`, `IntentBadge`, `ProjectTags`, `ThemeToggle`
- API client: `gloo-net` fetch to Axum JSON endpoints
- Removed: `src/api/web.rs` (Askama), `templates/`, old `static/` (htmx.min.js)
- Axum serves WASM via `ServeDir` + SPA fallback (`UI_DIST` env var)
- Portfolio theme: Atkinson font, `#2337ff` accent, paulxie design tokens
- **Crate:** code-rag-ui, code-rag-chat (routing changes)

### V2.5: GitHub Pages Demo + Engine Extraction [COMPLETE]

**Goal:** Deploy fully static demo to GitHub Pages. Run entire RAG pipeline in-browser via WASM.

**New crate: `code-rag-engine`**
- Extracted pure, platform-agnostic algorithms from `src/engine/`: intent classification, context building, config, scored retrieval
- No I/O, no HTTP — compiles to both native and `wasm32-unknown-unknown`
- `IntentClassifier::build(closure)` — caller provides embed function; decoupled from concrete `Embedder`
- `IntentClassifier::from_prototypes()` — load pre-computed embeddings (WASM standalone)
- `src/engine/` now re-exports from `code-rag-engine`, keeps only I/O-bound `retrieve()` and `LlmClient`
- 25 tests (includes 3 closure-based classifier tests)

**New feature: `code-rag-ui --features standalone`**
- In-browser RAG: transformers.js embeddings, brute-force L2 search, code-rag-engine classification + context
- `standalone_api.rs`: full pipeline (with Gemini) + rag-only (without LLM, works unauthenticated)
- `auth.rs`: OAuth2 PKCE flow, API key input, localStorage persistence
- `embedder.rs`: wasm-bindgen bridge to transformers.js via `window.__codeRagEmbedQuery()`

**New subcommand: `code-raptor export`**
- Reads all 4 chunk types + embeddings from LanceDB
- Pre-computes intent prototype embeddings
- Outputs `ChunkIndex` JSON for standalone WASM demo

**CI/CD: `.github/workflows/gh-pages.yml`**
- Config-driven ingestion targets (`config/targets.json`)
- Builds with `--features standalone`, deploys to GitHub Pages

**Test Results:** 135 tests pass (up from 132)
- **Crates:** code-rag-engine (new), code-rag-ui (standalone feature), code-raptor (export), code-rag-chat (re-exports)

---

## V3: Quality Harness

**Goal:** Establish quantitative testing infrastructure before Track parallelization.

**Estimated effort:** ~1 week total

| Item | Effort | Notes |
|------|--------|-------|
| V3.1 Test Dataset | 2-3 days | Writing 20-50 queries with expected results |
| V3.2 Recall Script | 1-2 days | Query runner + metrics calculation |
| V3.3 Baseline Docs | 1 day | Run script, document results |
| V3.4 Embedding Eval | 2-3 days | Code-specific model benchmark |

### V3.1: Retrieval Test Dataset
- JSON file: `test_queries.json`
- 20-50 queries covering: overview, implementation, relationship intents
- Format: `{"query": "...", "expected_files": ["..."], "intent": "..."}`
- Include hero queries from V1 and V2
- **Crate:** code-rag-chat (test fixtures)

### V3.2: Recall Measurement Script
- Script that measures recall@5, recall@10 for each query
- Outputs: per-query results + aggregate metrics
- Run after each milestone to detect regressions
- **Crate:** code-rag-chat

### V3.3: Baseline Documentation
- Run V3.2 against V2 index
- Document: recall, p95 latency, tokens/query
- Establishes comparison point for Track improvements

### V3 Success Criteria
- Test dataset covers all intent categories
- Baseline recall@5 documented
- Script runs in <60s for full test suite
- Code embedding model evaluated; decision documented

---

# Track A: Hierarchy (Top-Down Architecture)

**Goal:** Answer architecture-level questions about unfamiliar codebases.

**Estimated effort:** ~1.5-2 weeks

| Item | Effort | Notes |
|------|--------|-------|
| A1 Text Module Consolidation | 1-2 days | Deduplicate WASM/native code into code-rag-engine |
| A2 Folder Embeddings | 3-4 days | New chunk type, template-based summarization |
| A3 Collapsed-Tree Routing | 1-2 days | Extend routing table — modular, works with any chunk type combo |
| A4 File Embeddings | 2-3 days | Similar pattern to A2; lights up in routing automatically |
| A5 Repo Summaries | 2-3 days | README/manifest parsing, template-based |

**Ordering rationale:** Consolidation (A1) first so new chunk types share code from day one. Routing (A3) early so each subsequent chunk type is harness-testable immediately. Routing is modular — missing chunk types produce empty results (limit > 0 but table empty = no-op). Each new type "lights up" without routing changes.

### Design Rationale (SOTA Research)

**Multi-level querying:** RAPTOR (ICLR 2024) proved "collapsed tree" (query all levels simultaneously, fuse results) outperforms single-level routing by ~20%. The current system already does this — searches all chunk types per query with intent-varied limits via RRF. A2 extends this pattern, not replaces it.

**No sub-function chunks:** Functions are natural semantic units. Sub-function blocks (if/else, match arms) lack standalone meaning. MGS3 and cAST show gains but for code completion, not code understanding. Large functions (100+ lines) are a chunking concern, not hierarchy — handle separately if harness shows need.

**Embedding granularity mismatch:** Mitigated by (1) embedding summaries not raw content (~100-300 tokens, comparable to CodeChunk's ~200-500), and (2) per-type search + RRF fusion (rank-based, never compares scores across types). Monitor recall@K per chunk type post-A.

**WASM compatibility:** All summaries are template-based (deterministic, no LLM). Generated at CI ingestion time by code-raptor. Exported to index.json alongside existing chunks. No new CI secrets or dependencies.

**Hierarchy levels:**
```
Repo Summary  (CrateChunk / ReadmeChunk — already exist)
    └── Folder   (FolderChunk — new)
        └── File     (FileChunk — new)
            └── Function (CodeChunk — already exists)
```

### A1: Text Module Consolidation
- Create `code-rag-engine::text` module (pure, no I/O, compiles to WASM + native)
- Move into it:
  - `tokenize()` — from code-rag-ui/text_search.rs and code-raptor/export.rs (3 copies → 1)
  - `IdfTable` struct + `idf()` + `build()` — from code-rag-ui/text_search.rs and code-raptor/export.rs
  - `BM25 scoring` algorithm — from code-rag-ui/text_search.rs
  - `build_searchable_text()` + `split_camel_case()` — from code-rag-store/vector_store.rs
  - Intent prototype texts — from code-raptor/export.rs (duplicated from intent.rs)
- Remove dead `build_searchable_text()` copy from code-rag-ui/data.rs
- Update imports in code-rag-ui, code-raptor, code-rag-store
- **Crate:** code-rag-engine (new text module), updates to all consumers
- **Testable:** Existing harness + unit tests must pass unchanged (pure refactor, no behavior change)
- **Benefit:** A2+ folder/file searchable text and BM25 go into code-rag-engine from day one — zero new duplication

### A2: Folder-Level Embeddings
- New `FolderChunk` type in code-rag-types
- Template-based summary (deterministic, no LLM):
  ```
  Folder: {path}
  Contains: {file_count} files ({languages})
  Key types: {public structs/classes/traits from AST}
  Key functions: {top public functions by name}
  Subfolders: {subfolder list}
  ```
- Embed template text (~100-200 tokens)
- **Crate:** code-raptor (types in code-rag-types)
- **Testable:** Unit tests for template correctness, ingestion roundtrip, export includes folder_chunks

### A3: Collapsed-Tree Routing
- **Modular design:** works with any combination of chunk types present
  - Search arms return empty results when a chunk table has no data
  - Limits > 0 for missing types are harmless no-ops
  - Each new chunk type registers once; routing table needs no changes per type
- Add `folder_limit` and `file_limit` fields to `RetrievalConfig`
- All queries search all levels; intent controls the mix:

| Intent         | code | folder | file | readme | crate | module_doc |
|----------------|------|--------|------|--------|-------|------------|
| Overview       | 3    | 4      | 3    | 3      | 3     | 3          |
| Implementation | 5    | 1      | 2    | 1      | 1     | 2          |
| Relationship   | 5    | 2      | 2    | 1      | 2     | 2          |
| Comparison     | 5    | 2      | 2    | 2      | 3     | 2          |

- Add `folder_vec` and `file_vec` boolean arms to `ArmPolicy` (always true initially)
- BM25 and rerank policies for folder/file TBD after harness evaluation
- **Crate:** code-rag-engine (routing table), code-rag-chat (retriever), code-rag-ui (WASM search)
- **Testable after A2+A3:**
  - Add ~5 folder-level test queries to test_queries.json
  - Full harness run: folder hero queries should now hit FolderChunks
  - Regression check: existing recall@5 for Implementation/Relationship should not drop
  - WASM: `ChunkIndex` loads folder_chunks, search arms produce results

### A4: File-Level Embeddings
- New `FileChunk` type in code-rag-types
- Template-based summary (deterministic, no LLM):
  ```
  File: {path} ({language})
  Exports: {public functions, structs, classes}
  Imports: {external dependencies}
  Purpose: {inferred from module doc / first docstring / filename}
  ```
- Embed template text (~100-200 tokens)
- **Shared with C1:** `extract_file_imports` on `LanguageHandler` trait is needed by both A1.4 (FileChunk "Imports" template field) and C1 (cross-file call resolution). Whichever track runs first builds it; the other reuses.
- **Crate:** code-raptor (types in code-rag-types)
- **Testable immediately (routing already in place):**
  - Add ~5 file-level test queries to test_queries.json
  - Harness run: file queries hit FileChunks; folder queries still work; code queries unaffected
  - Check if Overview recall improves with both folder + file chunks active

### A5: Repo Summaries
- Template-based only: extract from README + Cargo.toml/package.json + directory structure
- Extract tech stack from dependencies
- Identify entry points
- No LLM summarization (expensive to re-ingest, non-deterministic)
- LLM enhancement noted as theoretical option but not pursued
- **Crate:** code-raptor
- **Testable immediately (routing already in place):**
  - Add architecture-level test queries (e.g., "What are the main components?")
  - Harness run: full hierarchy active, compare aggregate metrics vs V3.3 baseline
  - Final A quality gate: Overview recall target, no Implementation regression

### WASM/Native Code Sharing

After A1 consolidation, new Track A logic lives in `code-rag-engine` (shared). Platform layers are thin wrappers:
- `code-rag-store`: wraps engine text/search logic with LanceDB queries (native)
- `code-rag-ui`: wraps engine text/search logic with brute-force vectors (WASM)
- `code-raptor export`: uses engine for IDF + searchable text, adds folder/file tables to JSON

**WASM changes distributed across steps (not a separate block):**
- A2: `ChunkIndex` gains `folder_chunks`, `search_folder_arm()` uses engine's BM25
- A3: Routing table + ArmPolicy changes (already WASM-compatible via code-rag-engine)
- A4: `ChunkIndex` gains `file_chunks`, `search_file_arm()` uses engine's BM25
- A5: No additional WASM changes needed
- index.json size impact: ~250 extra chunks per repo, minimal

**Deliverable:** "What does the engine/ folder do?" returns meaningful answer.

### A Hero Queries
- "What does the engine/ folder do?" → Returns folder-level summary
- "How is code-rag-chat organized?" → Returns architecture overview
- "What are the main components?" → Lists crates and their purposes

### Key References
- RAPTOR (ICLR 2024) — collapsed tree > layer traversal
- cAST (CMU, arXiv 2506.15655) — AST-aware chunking for code
- Code-Craft (arXiv 2504.08975) — hierarchical graph summarization
- AI21 query-dependent chunking — multi-scale indexing gains 1-37%
- HEAL (arXiv 2412.04661) — hierarchical embedding alignment loss

**Maps to Vision:** Improvement #4 (Hierarchical Embedding) + #5 (Repo Summaries)

---

# Track B: Search Precision

Independent track. Can run in parallel with Tracks A, C, and D.

**Track total:** ~3-4 weeks

---

## B1: Cross-Encoder Reranking

**Estimated effort:** 2-3 days

**Goal:** Add second-stage reranking to improve precision without changing indexing. Industry-standard technique yielding 20-35% recall improvement.

**Architecture:**
- Retrieve top-20 results from LanceDB (current single-stage vector search)
- Score each (query, chunk_text) pair with a cross-encoder model
- Re-sort by cross-encoder score, take top-K per intent config
- Cross-encoder runs locally via ONNX Runtime

**Model options:**
- `ms-marco-MiniLM-L-6-v2` (fast, ~200ms for 20 pairs, well-tested)
- `BGE-reranker-base` (better accuracy, ~400ms)
- Start with MiniLM, benchmark via harness

**Integration point:** Between `retrieve()` and `build_context()` in the query pipeline. New `rerank()` function in code-rag-engine (pure, no I/O — takes query + scored chunks, returns re-scored chunks).

**Crate:** code-rag-engine (reranking logic), code-rag-chat (ONNX model loading)

### B1 Hero Query
- Queries where the correct file is retrieved at rank 6-20 but not top-5 → reranking promotes it

---

## B2: Hybrid Search (BM25 + Semantic)

**Estimated effort:** 3-5 days
- Combine lexical (BM25) with vector similarity
- Boost exact identifier matches
- LanceDB supports both natively
- **Crate:** code-rag-chat

### Fusion Approach
- Use Reciprocal Rank Fusion (RRF) to combine BM25 and semantic scores
- Weight by query intent:
  - Identifier queries (exact names) → boost BM25 weight
  - Conceptual queries ("how does X work") → boost semantic weight
- Configurable weights per intent category

**Intent-specific weight guidance (informed by baseline metrics):**
- `Relationship` queries (0.38 recall): BM25 weight near zero — conceptual, not lexical
- `Implementation` queries with identifiers: BM25 weight dominant — exact match matters
- `Overview` queries: balanced weights
- Low-confidence classifications (below threshold): equal weights as safe default
- Consider using classifier confidence score to modulate fusion dynamically

**Deliverable:** "Show me UserService" finds exact match.

### B2 Hero Query
- "Show me Retriever" → Exact match (not semantically similar alternatives)

---

## B3: Declaration Signatures + searchable_text + Hybrid Re-enablement ✅ DONE (2026-04-05)

- AST-extracted declaration signatures for functions + structs/enums/traits/interfaces/classes (Rust, Python, TypeScript)
- `searchable_text` column: `identifier(2x) + camelCase_split + signature + docstring` — high-signal BM25 target
- FTS index retargeted from `code_content` to `searchable_text`
- Hybrid search re-enabled (was disabled after B2 regression)
- Per-intent empirical gating via 4×4 space search
- **Crates:** code-rag-types, code-raptor, code-rag-store, code-rag-engine, code-rag-ui

See `development_log.md` for results and per-intent gating rationale.

---

## B4: Intent Classifier Improvement

**Estimated effort:** 0.5-1 day

- Expand comparison prototypes (cover "difference between X and Y", "X vs Y", "X differs from Y" patterns)
- Tune classification threshold
- Optional keyword pre-filter for explicit comparison patterns
- **Objective:** raise classifier accuracy from 58% → ≥75% on the harness test set. Classification accuracy only — downstream retrieval gains are secondary.
- **Motivation:** B3 ground-truth harness exposed 58% classifier accuracy as a bottleneck. 3 of 5 Comparison queries misclassified as overview/relationship. Intent is a first-class pipeline component with its own metric ceiling.
- **Crate:** code-rag-engine (intent.rs — data-only change)

---

## B5: Dual Embeddings (Signature + Body)

**Estimated effort:** 2-3 days

- Two embeddings per code chunk: `signature_embedding` (short, structural) + `body_embedding` (pre-B3 format, behavioral)
- Query-time fusion via RRF or intent-weighted combination
- **Motivation:** B3 empirical result showed signatures in vector embeddings regress Comparison queries. Dual embeddings isolate signature BM25 value without polluting body vector search. B4 (intent classifier) is run first because part of the Comparison drop comes from misrouting, not embedding pollution.
- **Precedent:** ColBERT, Jina Code v2, Qdrant/Weaviate named vectors, Sourcegraph multi-index
- **Crates:** code-rag-types, code-rag-store, code-rag-engine

---

# Track C: Relationship Graph

Independent track. Can run in parallel with Tracks A, B, and D.

**Track total:** ~2-3 weeks

---

## C1: Graph RAG

**Estimated effort:** 11-14 days

End-to-end call graph: extract edges (same-file + cross-file), persist in LanceDB, query via graph traversal, export to WASM standalone demo. One implementation pass — no separate same-file/cross-file/query phases (they share the same identifier resolution work and deliver no value independently).

- Build global identifier→chunk_id index after parsing all files
- Add `extract_file_imports` to `LanguageHandler` for cross-file resolution (Rust `use`, Python `import`, TS `import`)
- Resolve calls via: same-file match → import-based → unique-global fallback → skip ambiguous
- Store as `CallEdge` in LanceDB (scalar table, no embeddings)
- `CallGraph` in-memory adjacency list in `code-rag-engine` (wasm-compatible, no petgraph)
- Relationship intent augments vector search with graph traversal (hybrid, graceful degradation)
- Export edges in `index.json` for GitHub Pages standalone demo
- Accept 80% cross-file accuracy. Skip: trait dispatch, duck typing, macros, closures-in-variables
- **Crates:** code-rag-types, code-rag-store, code-raptor, code-rag-engine, code-rag-chat, code-rag-ui

**Hero Queries:**
- "What calls the retrieve function?" → Returns accurate callers via graph traversal
- "Show the query flow" → Traces from API to retrieval via BFS path finding

**Maps to Vision:** Improvement #2, #9, #12, #13 (Call Graph phases)

---

## C2: Retrieval Gap Fixes — Comparison Decomposition, Path-Aware Embeddings, Graph Result Protection

**Prerequisite:** C1 (Graph RAG)

**Estimated effort:** 5-7 days

Three diagnosed retrieval gaps where the pipeline fails to surface known-correct indexed results. Each has a SOTA-validated fix.

- **C2a: Comparison query decomposition** — extract comparators, per-entity fetch, RRF merge. Fixes flat `code_limit` letting one side dominate.
- **C2b: Path-aware embeddings** — BM25 path injection (no re-embedding) + embedding path prepend. Fixes path-blind queries ("What is shared-py?").
- **C2c: Graph result protection** — provenance tagging, max-score dedup, reserved graph slots. Fixes graph-resolved chunks being dropped despite valid call edges.
- **Crates:** code-rag-engine (intent, fusion, graph), code-rag-store (embedding format, BM25), code-raptor (ingestion)

---

## C3: Graph Embeddings Research (Time-Boxed, Optional)

**Prerequisite:** C2 (Retrieval Gap Fixes)

**Estimated effort:** 3-5 days (TIME-BOXED)

**Goal:** Evaluate whether structural graph embeddings (Node2Vec or similar) improve relationship query recall beyond what C1 graph traversal + C2 fixes achieve.

- Fuse with semantic embeddings via RRF (new channel, not replacement for graph traversal)
- **Success criteria:** Relationship recall improves by >0.05 over C2 baseline, OR documented findings on why graph embeddings don't add value for code.
- **Crates:** code-rag-engine (fusion logic), code-raptor (graph embedding generation)

---

# Track D: Enrichment Pipeline

Independent track. Can run in parallel with Tracks A, B, C. Only prerequisite is V1.5 (docstring extraction, complete).

**Goal:** Make undocumented code searchable through generated descriptions.

**Estimated effort:** ~1-1.5 weeks

| Item | Effort | Notes |
|------|--------|-------|
| D1 Docstring Generation | 3-4 days | LLM integration, caching, source flags |
| D2 Type Inference | 2-3 days | Similar pattern to D1 |

### D1: Docstring Generation
- Generate when: no docstring in store, OR existing is marked `source: generated`
- Never overwrite docstrings not marked as `source: generated`
- Content-hash caching (regenerate `source: generated` only on code changes)
- Tiered models: Haiku for bulk, better model for central functions
- Store separately (never modify source)
- **Crate:** code-raptor

**Contextual Preamble (Anthropic's Contextual Retrieval technique):**
- For each CodeChunk, generate a 50-100 token preamble situating it in the codebase: "This function is part of the ingestion pipeline in code-raptor. It handles..."
- Preamble prepended to embedding text in `format_code_for_embedding()`, NOT stored on the chunk struct (ephemeral, like V2.1 calls)
- Same LLM call pattern as docstring generation (Haiku for bulk, batch together)
- Reduces failed retrievals by up to 49% (67% combined with B1 reranking)
- Requires `--full` re-index (same as docstring generation)

### D2: Type Inference for Python
- LLM-infer types for untyped Python functions
- Same pattern as docstring generation
- Store with `source: generated` flag
- **Crate:** code-raptor

**Deliverable:** Undocumented third-party code returns useful search results.

### D Hero Queries
- Query undocumented function → Returns generated description
- "What does [third-party function] do?" → Meaningful answer despite no docstring

**Maps to Vision:** Improvement #7 (Docstring Generation) + #8 (Type Generation)

---

# Track R: RAPTOR Research (Bottom-Up Architecture)

Research track. Starts after Track D completes (needs D-generated summaries for clustering input). Track A hierarchy needed only for R5 architecture comparison.

**Goal:** Validate emergent architecture discovery via clustering. Time-boxed research sprint.

**Estimated effort:** 2 weeks (TIME-BOXED)

| Item | Effort | Notes |
|------|--------|-------|
| R1 Clustering Experiments | 3-4 days | Algorithm comparison, parameter tuning |
| R2 Cross-Cutting Handling | 2-3 days | Strategy evaluation |
| R3 Cluster Summarization | 2-3 days | LLM summarization, ClusterChunk type |
| R4 Recursive Abstraction | 2-3 days | Only if R1-R3 succeed |
| R5 Architecture Comparison | 2 days | Query routing to both views |

**Risk:** High variance. May conclude "doesn't work for code" - that's a valid outcome.

**Prerequisites:**
- Track D (Enrichment) for clustering on generated summaries instead of raw code
- Track A (Hierarchy) for architecture comparison (R5 only)

### R1: Clustering Experiments
- Cluster on D-generated summaries (not raw code embeddings)
- Experiment with algorithms:
  - HDBSCAN (handles varying density, noise)
  - Hierarchical clustering
  - Spectral clustering
- Evaluate cluster coherence
- **Crate:** code-raptor

### R2: Cross-Cutting Concern Handling
- **Problem:** Logging, error handling cluster together but aren't a "module"
- Strategies:
  - Exclude common patterns
  - Separate cluster type for cross-cutting
  - Accept as emergent insight
- Document findings

### R3: Cluster Summarization
- LLM-summarize each cluster
- "These N functions handle authentication..."
- New `ClusterChunk` type
- **Crate:** code-raptor (types in code-rag-types)

### R4: Recursive Abstraction (If Phase 1 Succeeds)
- Embed cluster summaries
- Cluster again, summarize
- Repeat until convergence or max depth
- Result: emergent architectural tree

### R5: Architecture Comparison
- **Requires:** Track A hierarchy (FolderChunk, FileChunk)
- Query routing to both views
- "What's the architecture?" → top-down (A) + bottom-up (R)
- Highlight discrepancies (architectural drift detection)
- **Crate:** code-rag-chat

### Research Questions
- Best clustering algorithm for code semantics?
- Optimal cluster size / recursion depth?
- How to evaluate quality of emergent structure?
- How to handle cross-cutting concerns?

**Deliverable:** Validated clustering approach with evaluation results, OR documented learnings on why it doesn't work.

**Maps to Vision:** Improvement #14 (Code Topology / RAPTOR)

**Success Criteria:**
- Clusters are semantically coherent (human evaluation + cluster purity vs folder structure)
- Emergent structure reveals non-obvious groupings
- Comparison with folder structure provides insight

---

## Crate Mapping

| Improvement | Crate |
|-------------|-------|
| Schema foundation (V1.1) | code-rag-types, code-rag-store |
| LanguageHandler refactor (V1.2) | code-raptor |
| Incremental ingestion (V1.3) | code-rag-types, code-rag-store, code-raptor |
| TypeScript support (V1.4) | code-raptor |
| Docstring extraction (V1.5) | code-raptor |
| Inline call context (V2.1) | code-raptor |
| Intent classification + query routing (V2.2) | code-rag-engine, code-rag-chat |
| Retrieval traces (V2.3) | code-rag-engine, code-rag-chat, code-rag-store |
| Leptos migration (V2.4) | code-rag-ui, code-rag-chat |
| GitHub Pages demo + engine extraction (V2.5) | code-rag-engine, code-rag-ui, code-raptor |
| Quality harness (V3) | code-rag-chat |
| Docstring generation | code-raptor |
| Hierarchical embeddings | code-raptor |
| Graph RAG (C1) | code-rag-types, code-rag-store, code-raptor, code-rag-engine, code-rag-chat, code-rag-ui |
| Retrieval gap fixes (C2) | code-rag-engine, code-rag-store, code-raptor |
| Type generation | code-raptor |
| RAPTOR clustering | code-raptor |
| Repo summaries | code-raptor |
| Cross-encoder reranking (B1) | code-rag-engine, code-rag-chat |
| Hybrid search | code-rag-chat |
| Graph query interface | code-rag-chat |
| Code embedding evaluation (V3.4) | code-rag-store |
| Graph embeddings research (C3) | code-rag-engine, code-raptor |
| HyDE query transformation (hypothetical) | code-rag-engine, code-rag-chat |

---

## Success Metrics

| Milestone | Metric |
|-----------|--------|
| V1 [DONE] | Docstrings appear in results (Rust, Python, TypeScript); TypeScript files indexed with docstrings; re-ingestion <30s for unchanged code; incremental ingestion skips unchanged files; `--full`/`--dry-run`/`--project-name` CLI flags work; 97 tests pass |
| V2 [DONE] | Queries route by intent (cosine similarity); retrieval sources with relevance scores shown; call context in embeddings; Leptos WASM frontend; GitHub Pages standalone demo; code-rag-engine shared algorithms; 135 tests pass |
| V3 | Test dataset with 20+ queries; baseline recall@5 documented; regression script runs <60s |
| V3.4 | Code embedding model evaluated; decision documented |
| A | "What does engine/ do?" returns coherent answer |
| D | Undocumented code has generated descriptions in search |
| R | Clustering produces meaningful emergent structure (or documented why not) |
| B1 | Reranking improves recall@5 by >10% over baseline |
| B2-B3 | "Show me UserService" finds exact match |
| C1-C2 | "What calls X?" returns accurate results; comparison queries cover both sides |
| C3 | Graph embeddings evaluated; decision documented |

---

## What We're NOT Doing

| Feature | Rationale |
|---------|-----------|
| Real-time code completion | Not our niche (Copilot/Cursor) |
| Code generation | Focus is understanding, not generation |
| IDE integration | CLI/chat interface first |
| Multi-language parity | Rust + Python priority, others later |
| Multi-repo queries | Deferred; foundation exists via `project_name` |

---

## Testing Strategy

### Levels of Testing

| Level | What | How | When |
|-------|------|-----|------|
| **Unit tests** | Individual components (parser, embedder, retriever) | Standard Rust tests | Throughout |
| **Integration tests** | End-to-end query → response | Test fixtures with known codebases | Throughout |
| **Hero queries** | Manual validation of key scenarios | 5-10 queries per milestone | V1, V2 |
| **Quantitative harness** | Automated recall@K measurement | V3 test dataset + script | V3 onwards |

### Testing Progression

| Phase | Testing Approach |
|-------|------------------|
| **V1-V2** | Hero queries (manual). Validate concept works before investing in automation. |
| **V3** | Build quantitative harness. Establish baseline metrics. |
| **Tracks** | Run harness after each milestone. Detect regressions. Track improvements. |

### V3 Quality Harness (Details in V3 section)

- **Test dataset:** 20-50 queries with expected files (`test_queries.json`)
- **Metrics:** recall@5, recall@10, p95 latency, tokens/query
- **Automation:** Script runs in <60s, outputs per-query + aggregate results

### Self-Reference Verification

Ensure code-rag is always in the ingested codebase. The hero query "How does the retriever work?" should return `retriever.rs` from code-rag-chat itself. This meta-demonstration is a strong portfolio signal.

---

# Research Ideas

Ideas informed by 2025-2026 RAG advancements. Not scheduled — evaluate after Tracks complete.

---

## Hypothetical: HyDE Query Transformation

**Status:** Idea — not scheduled, no track number assigned. Depends on B1 (reranking) and B2 (hybrid search) being in place first.

**Prerequisite:** B1 (Cross-Encoder Reranking) + B2 (Hybrid Search) — HyDE benefits from reranking as a safety net against hallucination, and from hybrid search for exact-match fallback.

**The problem:** Embedding a *question* ("How does the retriever build context?") and embedding a *code snippet* (`fn build_context(...)`) produce vectors in different regions of embedding space. Raw queries live in "question space" while indexed chunks live in "code/document space". This gap is larger for code than for text-to-text RAG.

**HyDE (Hypothetical Document Embeddings):**
1. User asks: "How does the retriever build context?"
2. Before embedding, send to a fast LLM (Haiku/Gemini Flash): "Write a short code snippet or description that would answer this question about a Rust codebase"
3. LLM generates: "The build_context function in context.rs assembles markdown sections from scored chunks — crate structure first, then module docs, relevant code snippets, and README excerpts, each formatted with headers and truncated to fit the context window."
4. Embed the *hypothetical answer* instead of (or alongside) the raw query
5. This pseudo-document is now in "document space", much closer to the actual code embedding

**Why it helps code RAG specifically:**
- For relationship queries ("what calls X?"), HyDE generates a hypothetical call chain closer to actual call-site code than the abstract question
- For overview queries ("what does engine/ do?"), HyDE generates a summary close to actual README/module doc embeddings
- Largest benefit for intent categories where query↔document embedding gap is widest

**Risks and mitigations:**
- **Hallucination**: LLM might generate code patterns that don't exist → retrieves irrelevant chunks. **Mitigation**: B1 reranking scores hallucinated matches low
- **Latency**: One LLM call (~200ms with Haiku). Can run in parallel with intent classification
- **Dual embedding strategy**: Embed both raw query AND HyDE output, search with both, merge via RRF — hedges against hallucination

**When NOT to use HyDE:**
- Exact identifier queries ("show me Retriever") — B2 hybrid search handles these better
- Gate on intent: only apply for `Overview` and `Relationship` intents, skip for `Implementation` with exact identifiers
