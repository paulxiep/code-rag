# CodeRAG: Vertical Development Plan

Ideated with LLM assistance, structured for agile-friendly milestones.

Refer to [project vision](project-vision.md) for full improvement ideas and [architecture](architecture.md) for technical design.

---

## Philosophy

**Build vertically, not horizontally.**

Each iteration delivers a thin slice through both crates (code-raptor + portfolio-rag-chat). Every version produces something *runnable* and *demonstrable*.

**Value proposition: Decouple knowledge from reasoning.**

Rich, structured retrieval amplifies *any* model—cheap or frontier. By offloading "what context is relevant" to the retrieval layer, the model's complexity budget can be spent on reasoning, multi-step workflows, or tool orchestration. This scales independently of model choice: better retrieval benefits Haiku and Opus alike.

---

## Guiding Mantra

> **"Declarative, Modular, SoC"**

Every implementation decision should be evaluated against these three principles:

| Principle | Meaning | Example |
|-----------|---------|---------|
| **Declarative** | Describe *what*, not *how*. Config over code. Data-driven behavior. | Chunk types declare their schema; retrieval strategies defined by config, not hardcoded. Intent routing rules are data, not if-else chains. |
| **Modular** | Components are self-contained, swappable, and independently testable. | code-raptor and portfolio-rag-chat share no code, only LanceDB schema. Swap HDBSCAN for hierarchical clustering without touching summarization. |
| **SoC** (Separation of Concerns) | Each module has ONE job. No god objects. Clear boundaries. | code-raptor = indexing. portfolio-rag-chat = querying. Types live in coderag-types. No crate does two things. |

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
code-raptor (producer)          portfolio-rag-chat (consumer)
    │                                   │
    │ writes chunks                     │ reads chunks
    ▼                                   ▼
              [LanceDB Schema]
              - CodeChunk (function-level)
              - FolderChunk (A1)
              - FileChunk (A1)
              - ClusterChunk (A3)
              - CallEdge (C1)
```

---

## Dependency Graph

### code-raptor (Indexing) Dependencies

```
Fix docstring extraction
    └──► Docstring generation [must extract before generate]
            └──► RAPTOR clustering [clusters on generated summaries]

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

RAPTOR clustering ─────────────── needs A2 enrichment + A1 hierarchy
    └──► Architecture comparison [requires A1 hierarchy]
```

### portfolio-rag-chat (Query) Dependencies

```
Intent classification + query routing
    └──► Hierarchical query routing [requires folder/file chunks]

Hybrid search ─────────────────── independent (query-side)

Graph query interface ─────────── requires call graph data
```

### Parallelization Opportunities

| Can run in parallel | Rationale |
|---------------------|-----------|
| Track A + Track B + Track C | Independent after V3 |
| A1 + B1 + C1 | All can start after V3 completes |
| V2.2 + V2.3 | All portfolio-rag-chat, no dependencies |
| Folder/File embeddings + Hybrid search | Indexing vs query |

---

## Iteration Structure

```
V1 (Indexing Foundation) ─── incremental ingestion + docstrings
 │
 ▼
V2 (Query Intelligence) ─── intent routing + retrieval traces
 │
 ▼
V3 (Quality Harness) ─── quantitative testing infrastructure
 │
 ├──► Track A: Semantic Understanding (sequential)
 │       A1: Hierarchy → A2: Enrichment → A3: RAPTOR
 │
 ├──► Track B: Search Precision (independent)
 │       B1 → B2 → B3
 │
 └──► Track C: Relationship Graph (independent)
         C1 → C2 → C3
```

V1 → V2 → V3 are sequential. Tracks A, B, C can run in parallel after V3. Prioritize based on user needs.

### Effort Summary

| Phase | Effort | Cumulative |
|-------|--------|------------|
| **V1** (Indexing) | 2.5-3 weeks | 2.5-3 weeks |
| **V2** (Query) | 1 week | 2.5-3 weeks |
| **V3** (Testing) | 1 week | 3.5-4 weeks |
| **Track A** (A1→A2→A3) | 4-5 weeks | — |
| **Track B** (B1→B2→B3) | 2-3 weeks | — |
| **Track C** (C1→C2→C3) | 2-4 weeks | — |

**If running Tracks in parallel:** V1+V2+V3 (~4 weeks) + longest Track (~5 weeks) = **~9 weeks to full feature set**

**If running Tracks sequentially:** ~13-16 weeks total

### Track Priority (Portfolio Demo Context)

For portfolio demonstrations, hirers ask architecture questions first:

| Track | Priority | Rationale |
|-------|----------|-----------|
| **A1 (Hierarchy)** | High | "What does this folder do?" is a likely first question |
| **B1 (Hybrid search)** | High | Precision for exact identifier queries |
| **A2 (Enrichment)** | Medium | Helps with undocumented code |
| **C1 (Same-file edges)** | Medium | Basic relationship queries |
| **A3 (RAPTOR)** | Research | Impressive if successful, time-boxed |

---

## V1: Indexing Foundation [COMPLETE]

**Goal:** Enable fast iteration, clean language abstraction, and fix docstring extraction. All code-raptor + coderag-store work.

| Item | Status | Notes |
|------|--------|-------|
| V1.1 Schema Foundation | Done | UUID, content_hash, delete API, List deps, model version |
| V1.2 LanguageHandler Refactor | Done | Pure refactor: trait + registry, docstring stays None |
| V1.3 Incremental Ingestion | Done | File-level hashing, three-layer architecture, schema tightening |
| V1.4 TypeScript Support | Done | TypeScriptHandler with JSDoc extract_docstring |
| V1.5 Docstring Extraction | Done | Parser wiring, Rust + Python extraction, TypeScript activation |

### V1.1: Schema Foundation (FIRST)

**Why:** Current schema lacks fields needed for incremental operations and has design debt that compounds in later phases.

**Changes to coderag-types:**
- Add `chunk_id: String` (UUID) - stable foreign key for Track C call graph edges
- Add `content_hash: String` - SHA256 of code_content for change detection
- Add `embedding_model_version: String` - prevents silent embedding inconsistency

**Changes to coderag-store:**
- Add delete API: `delete_chunks_by_file()`, `delete_chunks_by_project()`
- Change `crate_chunks.dependencies` from CSV string to `List<Utf8>` - enables "what depends on X?" queries
- Update Arrow schemas for all 4 tables

**Crates:** coderag-types, coderag-store

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
- **Crates:** coderag-types, coderag-store, code-raptor

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

**Crate:** code-raptor, portfolio-rag-chat

**Deliverable:** Fast re-ingestion. Clean language abstraction. Docstrings in search results. TypeScript support with docstrings from day one. V1 milestone complete.

### V1 Hero Queries (Testing Checkpoint — Ready to Validate)
- "What is code-raptor?" → Explains ingestion pipeline with docstrings visible
- "How does the retriever work?" → Returns `retriever.rs` (self-reference verification)

---

## V2: Query Intelligence

**Goal:** Make queries smarter with intent routing and visible retrieval sources.

**Estimated effort:** ~1 week total


| Item | Effort | Notes |
|------|--------|-------|
| V2.1 Inline Call Context | 1 day | Ephemeral call extraction via tree-sitter, enriches embedding text (code-raptor) |
| V2.2 Intent Classification + Query Routing | 1-2 days | Declarative keyword rules, `QueryIntent` enum, `RoutingTable` HashMap (portfolio-rag-chat) |
| V2.3 Retrieval Traces | 1-2 days | `ScoredChunk<T>`, all chunk types as sources, relevance scores (both crates) |

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

**SoC rationale for ephemeral design:** `coderag-types` is the cross-crate data contract defining the LanceDB schema. Adding an ephemeral `calls` field would pollute the contract with embedding-pipeline-specific data. `CodeChunk` is used by reconcile, by the query-side retriever, and by context formatting — none need calls. Track C will store persistent call edges in a separate `call_edges` table.

**Per-language extraction:**
- **Rust:** `call_expression` → `identifier` (direct) or `field_expression > field_identifier` (method)
- **Python:** `call` → `identifier` (direct) or `attribute > identifier` (method)
- **TypeScript:** `call_expression` → `identifier` (direct) or `member_expression > property_identifier` (method)
- Each handler owns its AST semantics via private `collect_calls_recursive` helper

**Scope exclusions:** No macros (`macro_invocation` nodes, not `call_expression`), no variable-bound calls, no cross-file resolution (Track C scope).

**Breaking change:** `analyze_with_handler` return type changes from `Vec<CodeChunk>` to `Vec<(CodeChunk, Vec<String>)>`, requiring ~30 tests to add mechanical destructuring.

**Deployment:** Requires `code-raptor ingest <repo> --full` after deployment. `content_hash` is SHA256 of source file, not embedding text — incremental mode won't re-embed unchanged files.

**Crates affected:** code-raptor (`language.rs`, `languages/*.rs`, `parser.rs`, `mod.rs`, `main.rs`), coderag-store (`embedder.rs`)

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
- **Crate:** portfolio-rag-chat

### V2.3: Retrieval Traces
- Extract `_distance` from LanceDB, convert to relevance: `1.0 / (1.0 + distance)`
- `ScoredChunk<T>` generic wrapper pairs each chunk with relevance score
- Redesigned `SourceInfo`: `chunk_type`, `path`, `label`, `project`, `relevance`, `line`
- All 4 chunk types surfaced as sources, sorted by relevance descending
- `ChatResponse` gains `intent` field
- **Demo value:** Makes retrieval quality visible; differentiator from black-box tools
- **Crate:** portfolio-rag-chat, coderag-store

**Deliverable:** Intent-based routing. Visible retrieval sources. Embeddings include call context.

### V2 Hero Queries (Testing Checkpoint)
- "How does the chat endpoint work?" → intent: implementation, sources include handlers.rs with relevance %
- "What is code-raptor?" → intent: overview, sources show README + CrateChunks ranking higher
- Overview vs implementation queries produce visibly different source distributions

---

## V3: Quality Harness

**Goal:** Establish quantitative testing infrastructure before Track parallelization.

**Estimated effort:** ~1 week total

| Item | Effort | Notes |
|------|--------|-------|
| V3.1 Test Dataset | 2-3 days | Writing 20-50 queries with expected results |
| V3.2 Recall Script | 1-2 days | Query runner + metrics calculation |
| V3.3 Baseline Docs | 1 day | Run script, document results |

### V3.1: Retrieval Test Dataset
- JSON file: `test_queries.json`
- 20-50 queries covering: overview, implementation, relationship intents
- Format: `{"query": "...", "expected_files": ["..."], "intent": "..."}`
- Include hero queries from V1 and V2
- **Crate:** portfolio-rag-chat (test fixtures)

### V3.2: Recall Measurement Script
- Script that measures recall@5, recall@10 for each query
- Outputs: per-query results + aggregate metrics
- Run after each milestone to detect regressions
- **Crate:** portfolio-rag-chat

### V3.3: Baseline Documentation
- Run V3.2 against V2 index
- Document: recall, p95 latency, tokens/query
- Establishes comparison point for Track improvements

**Deliverable:** Quantitative baseline; regression safety net for Track development.

### V3 Success Criteria
- Test dataset covers all intent categories
- Baseline recall@5 documented
- Script runs in <60s for full test suite

---

# Track A: Semantic Understanding

Sequential: A1 → A2 → A3

---

## A1: Hierarchy (Top-Down Architecture)

**Goal:** Answer architecture-level questions about unfamiliar codebases.

**Estimated effort:** ~1.5-2 weeks

| Item | Effort | Notes |
|------|--------|-------|
| A1.1 Folder Embeddings | 3-4 days | New chunk type, summarization logic |
| A1.2 File Embeddings | 2-3 days | Similar pattern to A1.1 |
| A1.3 Repo Summaries | 2-3 days | README/manifest parsing, LLM summarization |
| A1.4 Hierarchical Routing | 2-3 days | Extend intent classifier |

### A1.1: Folder-Level Embeddings
- New `FolderChunk` type
- Auto-summarize folder contents (file list + inferred purpose)
- Embed folder summaries
- **Crate:** code-raptor (types in coderag-types)

### A1.2: File-Level Embeddings
- New `FileChunk` type
- Module-level summary (exports, main responsibilities)
- Embed file summaries
- **Crate:** code-raptor

### A1.3: Auto-Generate Repo Summaries
- Infer purpose from README, package manifests, structure
- Extract tech stack from dependencies
- Identify entry points
- LLM-summarize repos without good READMEs
- **Crate:** code-raptor

### A1.4: Hierarchical Query Routing
- Extend intent classifier for granularity
- `overview` queries → FolderChunk, repo summaries
- `module` queries → FileChunk
- `implementation` queries → CodeChunk
- **Crate:** portfolio-rag-chat

**Deliverable:** "What does the engine/ folder do?" returns meaningful answer.

### A1 Hero Queries
- "What does the engine/ folder do?" → Returns folder-level summary
- "How is portfolio-rag-chat organized?" → Returns architecture overview
- "What are the main components?" → Lists crates and their purposes

**Maps to Vision:** Improvement #4 (Hierarchical Embedding) + #5 (Repo Summaries)

---

## A2: Enrichment Pipeline

**Goal:** Make undocumented code searchable through generated descriptions.

**Estimated effort:** ~1-1.5 weeks

| Item | Effort | Notes |
|------|--------|-------|
| A2.1 Docstring Generation | 3-4 days | LLM integration, caching, source flags |
| A2.2 Type Inference | 2-3 days | Similar pattern to A2.1 |

### A2.1: Docstring Generation
- Generate when: no docstring in store, OR existing is marked `source: generated`
- Never overwrite docstrings not marked as `source: generated`
- Content-hash caching (regenerate `source: generated` only on code changes)
- Tiered models: Haiku for bulk, better model for central functions
- Store separately (never modify source)
- **Crate:** code-raptor

### A2.2: Type Inference for Python
- LLM-infer types for untyped Python functions
- Same pattern as docstring generation
- Store with `source: generated` flag
- **Crate:** code-raptor

**Deliverable:** Undocumented third-party code returns useful search results.

### A2 Hero Queries
- Query undocumented function → Returns generated description
- "What does [third-party function] do?" → Meaningful answer despite no docstring

**Maps to Vision:** Improvement #7 (Docstring Generation) + #8 (Type Generation)

---

## A3: RAPTOR Research (Bottom-Up Architecture)

**Goal:** Validate emergent architecture discovery via clustering. Time-boxed research sprint.

**Estimated effort:** 2 weeks (TIME-BOXED)

| Item | Effort | Notes |
|------|--------|-------|
| A3.1 Clustering Experiments | 3-4 days | Algorithm comparison, parameter tuning |
| A3.2 Cross-Cutting Handling | 2-3 days | Strategy evaluation |
| A3.3 Cluster Summarization | 2-3 days | LLM summarization, ClusterChunk type |
| A3.4 Recursive Abstraction | 2-3 days | Only if A3.1-A3.3 succeed |
| A3.5 Architecture Comparison | 2 days | Query routing to both views |

**Risk:** High variance. May conclude "doesn't work for code" - that's a valid outcome.

**Prerequisites:**
- A1 (Hierarchy) for architecture comparison
- A2 (Enrichment) for clustering on generated summaries instead of raw code

### A3.1: Clustering Experiments
- Cluster on A2-generated summaries (not raw code embeddings)
- Experiment with algorithms:
  - HDBSCAN (handles varying density, noise)
  - Hierarchical clustering
  - Spectral clustering
- Evaluate cluster coherence
- **Crate:** code-raptor

### A3.2: Cross-Cutting Concern Handling
- **Problem:** Logging, error handling cluster together but aren't a "module"
- Strategies:
  - Exclude common patterns
  - Separate cluster type for cross-cutting
  - Accept as emergent insight
- Document findings

### A3.3: Cluster Summarization
- LLM-summarize each cluster
- "These N functions handle authentication..."
- New `ClusterChunk` type
- **Crate:** code-raptor (types in coderag-types)

### A3.4: Recursive Abstraction (If Phase 1 Succeeds)
- Embed cluster summaries
- Cluster again, summarize
- Repeat until convergence or max depth
- Result: emergent architectural tree

### A3.5: Architecture Comparison
- **Requires:** A1 hierarchy (FolderChunk, FileChunk)
- Query routing to both views
- "What's the architecture?" → top-down (A1) + bottom-up (A3)
- Highlight discrepancies (architectural drift detection)
- **Crate:** portfolio-rag-chat

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

# Track B: Search Precision

Independent track. Can run in parallel with Track A and C.

**Track total:** ~2-3 weeks

---

## B1: Hybrid Search (BM25 + Semantic)

**Estimated effort:** 3-5 days
- Combine lexical (BM25) with vector similarity
- Boost exact identifier matches
- LanceDB supports both natively
- **Crate:** portfolio-rag-chat

### Fusion Approach
- Use Reciprocal Rank Fusion (RRF) to combine BM25 and semantic scores
- Weight by query intent:
  - Identifier queries (exact names) → boost BM25 weight
  - Conceptual queries ("how does X work") → boost semantic weight
- Configurable weights per intent category

**Deliverable:** "Show me UserService" finds exact match.

### B1 Hero Query
- "Show me Retriever" → Exact match (not semantically similar alternatives)

---

## B2: Function Signature Extraction

**Estimated effort:** 3-4 days

- Extract: function name, parameters, return type, type hints
- Store as structured metadata
- Include in embedding text: `fn process(input: &str) -> Result<Output>`
- **Crate:** code-raptor

---

## B3: Large Function Chunking

**Estimated effort:** 3-5 days
- Detect oversized functions (>N lines threshold)
- Chunk by logical blocks (loop, match arms, etc.)
- Maintain parent-child relationship
- **Crate:** code-raptor

**Maps to Vision:** Improvement #6 (Hybrid Search) + #8 (Signatures) + #10 (Chunking)

---

# Track C: Relationship Graph

Independent track. Can run in parallel with Track A and B.

**Track total:** ~2-4 weeks (C2 has high variance due to cross-file resolution complexity)

---

## C1: Same-File Call Edges

**Estimated effort:** 3-4 days
- Extract function→function edges within same file
- No import resolution needed
- Store as `CallEdge` in LanceDB
- **Crate:** code-raptor

---

## C2: Cross-File Call Graph

**Estimated effort:** 1-2 weeks (HIGH VARIANCE)

- Module-level imports first (File→File edges)
- Then function-level resolution
- Handle Rust traits/generics, Python duck typing gracefully (incomplete OK)
- **Crate:** code-raptor

**Risk:** Static analysis for dynamic languages can spiral. Accept 80% accuracy, don't chase edge cases.

---

## C3: Graph Query Interface

**Estimated effort:** 3-5 days
- New query type: `relationship`
- "What calls X?" → traverse CallEdge
- "Show the auth flow" → path finding
- **Crate:** portfolio-rag-chat

**Deliverable:** "What calls this function?" returns accurate callers.

### C1-C3 Hero Queries
- "What calls the retrieve function?" → Returns accurate callers
- "Show the query flow" → Traces from API to retrieval

**Maps to Vision:** Improvement #2, #9, #12, #13 (Call Graph phases)

---

## Crate Mapping

| Improvement | Crate |
|-------------|-------|
| Schema foundation (V1.1) | coderag-types, coderag-store |
| LanguageHandler refactor (V1.2) | code-raptor |
| Incremental ingestion (V1.3) | coderag-types, coderag-store, code-raptor |
| TypeScript support (V1.4) | code-raptor |
| Docstring extraction (V1.5) | code-raptor |
| Inline call context (V2.1) | code-raptor |
| Intent classification + query routing (V2.2) | portfolio-rag-chat |
| Retrieval traces (V2.3) | portfolio-rag-chat |
| Quality harness (V3) | portfolio-rag-chat |
| Docstring generation | code-raptor |
| Hierarchical embeddings | code-raptor |
| Call graph extraction | code-raptor |
| Type generation | code-raptor |
| RAPTOR clustering | code-raptor |
| Repo summaries | code-raptor |
| Hybrid search | portfolio-rag-chat |
| Graph query interface | portfolio-rag-chat |

---

## Success Metrics

| Milestone | Metric |
|-----------|--------|
| V1 [DONE] | Docstrings appear in results (Rust, Python, TypeScript); TypeScript files indexed with docstrings; re-ingestion <30s for unchanged code; incremental ingestion skips unchanged files; `--full`/`--dry-run`/`--project-name` CLI flags work; 97 tests pass |
| V2 | Queries route by type; retrieval sources shown; call context in embeddings |
| V3 | Test dataset with 20+ queries; baseline recall@5 documented; regression script runs <60s |
| A1 | "What does engine/ do?" returns coherent answer |
| A2 | Undocumented code has generated descriptions in search |
| A3 | Clustering produces meaningful emergent structure (or documented why not) |
| B1-B3 | "Show me UserService" finds exact match |
| C1-C3 | "What calls X?" returns accurate results |

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

Ensure portfolio-rag-chat is always in the ingested codebase. The hero query "How does the retriever work?" should return `retriever.rs` from portfolio-rag-chat itself. This meta-demonstration is a strong portfolio signal.
