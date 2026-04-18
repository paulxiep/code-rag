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
│  │  - FolderChunk (A2), FileChunk (A4)      │                    │
│  │  - CallEdge, ExportEdge                  │                    │
│  └──────────────────────────────────────────┘                    │
│                                                                   │
└───────────────────────────────────────────────────────────────────┘
```

## Crate Responsibilities

| Crate | Purpose | Key Files |
|-------|---------|-----------|
| `code-raptor` | Ingestion CLI — tree-sitter parsing, language handlers, incremental ingestion, data export | `ingestion/`, `export.rs`, `main.rs` |
| `code-rag-engine` | Shared algorithms — intent classification, context building, scoring, hierarchy summary rendering, single-source text primitives (pure, no I/O, compiles to wasm32) | `intent.rs`, `context.rs`, `config.rs`, `retriever.rs`, `text/` (A1), `folder.rs` (A2), `file.rs` (A4), `graph.rs`, `comparison.rs` |
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
                                  ▼
                         ┌──────────────────┐
                         │  Hybrid Search   │  vector + BM25(searchable_text)
                         │  (per ArmPolicy) │  → N-ary RRF fusion
                         └────────┬─────────┘
                                  │
                                  ▼
                         ┌──────────────────┐
                         │  Cross-Encoder   │  ms-marco-MiniLM-L-6-v2
                         │   Reranker       │  (4× over-retrieve on code,
                         │  (per ArmPolicy) │   sigmoid-normalized)
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

**Two query-side branches compose on top of the diagram above** (Track C):

- **Comparison pre-branch** (`code-rag-engine::comparison`, `extract_comparators` in `code-rag-engine::intent`): if `intent == Comparison` and ≥ 2 comparators are extractable via regex (`compare X and Y`, `X vs Y`, `differences between X and Y`, etc.), the retriever runs one body-vec sub-search per comparator (with the comparator name prepended to the original query), votes the dominant `project_name` across the original-query top-5, post-filters all results to that project, then RRF-fuses via `fuse_comparator_lists` and rewrites each surviving chunk's score to its **max-of-natural** body-vec relevance from any source list. Without max-of-natural, RRF outputs (~0.02–0.05) sink below distance-converted non-code arms (0.4–0.7) and crash comparison recall.
- **Graph augmentation + protection** (`code-rag-engine::graph`): on Relationship and Implementation intents, `graph_augment` resolves the query target against the call-graph identifier index (exact → graph index → partial), traverses callers/callees, and `merge_graph_chunks` returns the merged result list plus a `HashSet<String>` of graph-resolved chunk IDs. `detect_direction` then chooses a protection path: explicit direction ("what calls X / called by") → **SOTA routing** (graph chunks partitioned **out** of the rerank pipeline entirely, sorted by tier score, prepended to the reranked semantic chunks); ambiguous direction → **soft reserve** (`reserve_graph_slots` over-retains the code arm by `+5` and swaps demoted graph chunks back in). Both paths mirror line-for-line in the WASM standalone (`crates/code-rag-ui/src/standalone_api.rs`).

## Vector Schema (6 Vector Tables + 1 Scalar Table)

| Table | Content | Embedding Input | FTS / BM25 Target |
|-------|---------|-----------------|-------------------|
| `code_chunks` | Functions, classes, structs, traits, enums, interfaces | `identifier + docstring + code + calls` (body_vector) + `signature + language + docstring` (nullable `signature_vector`, shipped OFF) | `searchable_text` = identifier (2×) + camelCase split + signature + docstring |
| `folder_chunks` (A2) | One row per directory | `summary_text` from `code_rag_engine::folder::render_summary` — 5-line template: `Folder: path (module: basename) / Contains: N files (langs) / Key types: ... / Key functions: ... / Subfolders: ...` | `summary_text` |
| `file_chunks` (A4) | One row per source file | `summary_text` from `code_rag_engine::file::render_summary` — 4-line template: `File: path (module: basename, language) / Exports: ... / Imports: ... / Purpose: ...` | `summary_text` |
| `readme_chunks` | README.md files | `Project: name + content` | `content` |
| `crate_chunks` | Cargo.toml metadata | `Crate: name + description + dependencies` | natural text |
| `module_doc_chunks` | Module-level docs (`//!`) | `Module: name + doc_content` | natural text |
| `call_edges` | Caller→callee call relationships | (none — first scalar-only LanceDB table) | (none) |

`folder_chunks` / `file_chunks` Arrow schema uses **native `List<Utf8>`** for vec metadata fields (languages / key_types / key_functions / subfolders / exports / imports), matching the post-V1.1 `crate_chunks.dependencies` pattern — no JSON-encoded blobs. `summary_text` is persisted on the row (not re-rendered) so server-embedded bytes and browser BM25 bytes can never drift; the pure render function in `code-rag-engine` is the single source of truth.

`call_edges` schema: deterministic `edge_id`, caller/callee `chunk_id` + identifier + file, `project_name`, `resolution_tier: u8` (1=same_file, 2=import_based, 3=unique_global). Validated against LanceDB with a dedicated integration test before building the API. Queried directly by the graph traversal helpers in `code-rag-engine::graph` — no vector search.

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
┌─────────────────┐
│ Hierarchy Pass  │  build_folder_chunks (A2) + build_file_chunks (A4)
│                 │  Pure functions over CodeChunks + ImportsMap → templated summaries
│                 │  Embedded with the same fastembed model as code chunks
└────────┬────────┘
         │
         ▼
    LanceDB (6 vector tables + call_edges)
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
21. **Two-stage retrieval with cross-encoder**: bi-encoder (BGE-small) retrieves 4× over-candidates; cross-encoder (`ms-marco-MiniLM-L-6-v2`) scores each `(query, chunk)` pair with sigmoid-normalized logits. Intent-gated — web-passage model misjudges structural (relationship/comparison) queries, so it is switched off per-intent via `ArmPolicy`
22. **`searchable_text` as BM25 target, not `code_content`**: identifier repeated 2× (simulates field boosting since LanceDB supports single-column FTS) + camelCase split index-side + signature + docstring. BM25 on full code bodies was noisy; concentrating high-signal tokens recovered hybrid search
23. **Per-intent `ArmPolicy` as single source of truth**: `{body_vec, sig_vec, bm25, rerank}` per intent replaces scattered `matches!(intent, Comparison)` gates. Overview=hybrid+rerank, Implementation=rerank-only, Relationship=hybrid+rerank, Comparison=vector-only. Used by both native server `retriever` and browser `standalone_api`
24. **N-ary RRF fusion in `code-rag-engine::fusion`**: generic over arbitrary arm count (body + sig + bm25). Browser + server share the same fusion code
25. **Dual-vector schema shipped OFF**: `signature_vector` column populated but disabled. 8-config space sweep showed signature arm regressed every intent — short-text geometry mismatch with BGE-small (trained on passages) + sparse-arm RRF penalty (sig_vec null on ~25% of chunks). Column retained for future experiments
26. **k-NN prototype voting (k=3)**: classifier flattens all prototypes, takes top-k by similarity, similarity-weighted votes per intent. Robust to imbalanced prototype counts
27. **Comparison keyword pre-filter with adversarial guards**: hard-overrides to Comparison on `"difference between"`, `" vs "`, `compare`, etc., but rejects idioms (`"difference this makes"`) and identifier-embedded `_vs_` tokens (`transformer_vs_rnn.py`)
28. **Persistent call graph as scalar-only LanceDB table**: 3-tier resolver (same-file → import-based → unique-global) runs at ingest; queries hit `call_edges` directly via `code-rag-engine::graph`, not vector search. Self-edges and unresolvable ambiguous calls are skipped — no LLM-extracted noise. First LanceDB table without a vector column; validated with a dedicated integration test before building the API. AST-derived call graphs outperform LLM-extracted knowledge graphs for code (arXiv:2601.08773)
29. **Graph result protection — SOTA routing vs soft reserve**: explicit direction (`detect_direction` finds "what calls X / called by / depends on") → graph chunks partitioned **out** of the rerank pipeline entirely and prepended to reranked semantic results; ambiguous direction → graph chunks stay in the rerank pool, code arm over-retained by `+5`, `reserve_graph_slots` swaps demoted graph chunks back in. The browser-bundled `ms-marco-MiniLM` cross-encoder cannot be retrained for structural priors, so routing is structural, not score-based (matches Cody / LocAgent / GraphCoder; formal version in arXiv:2509.05980 GRACE)
30. **Comparison query decomposition with max-of-natural rescoring**: regex extracts comparators → per-comparator augmented sub-queries → vote-based dominant-project filter (top-1 was too brittle: `pre_classify_comparison`-style false positives) → RRF fuse → rewrite each chunk's score to its max natural body-vec relevance from any source list. Without max-of-natural, RRF outputs (~0.02–0.05) sink below distance-converted non-code arms (0.4–0.7) and crash comparison recall@5 from 0.65 to 0.31. SOTA bare-comparator sub-queries (LlamaIndex SubQuestionQueryEngine, RAG-Fusion) and per-sub-search BM25 (CodeRAG-Bench) **both regressed** on BGE-small + this corpus and are recorded as code comments to prevent re-running without measuring (re-test if the embedder is upgraded to BGE-base or jina-code)
31. **Test code exclusion at ingest (3-level)**: directory `tests/`, filename `test_*.py` / `*.test.ts`, and AST-walked `#[cfg(test)]` enclosing-mod detection via tree-sitter parent walk. Test functions containing query-like text (canonical case: `test_extract_target_term_what_calls` containing "What calls retrieve?" in its body) dominated both vector search and reranking before exclusion. Removed ~24% of chunks (3772 → 2861)
32. **Single text-primitives module — A1**: `code-rag-engine::text` collapses three pre-existing copies of `tokenize`, `IdfTable`, BM25 kernel, `build_searchable_text`, `split_camel_case`, and intent prototype text arrays into one wasm-pure module. Pure refactor (no behavior change, no re-ingest), but landing it before A2/A4 prevented every new chunk type from re-introducing the same drift B3 had previously burned a day debugging. Downstream crates (`code-rag-store`, `code-raptor`, `code-rag-ui`) became thin import-only consumers
33. **Hierarchy chunks via deterministic templates, not LLM — A2/A4**: `FolderChunk` and `FileChunk` are rendered by pure functions in `code-rag-engine::{folder,file}`. RAPTOR (Sarthi et al., ICLR 2024, arXiv:2401.18059) validates the "embed a subtree summary" pattern but is agnostic to *how* the summary is produced. Templates are reproducible (identical bytes on every re-ingest), cheap (no LLM API calls during CI), wasm-compatible, and built from facts already extracted at CodeChunk ingestion (public types/functions via `node_type` + signature-prefix visibility heuristic) plus C1's `ImportsMap`. `summary_text` is persisted on the row so server-embedded bytes and browser-BM25 bytes can never diverge — the render function is the single source of truth
34. **Dual label `(module: basename[, language])` in the template — A2/A4**: users phrase directory questions as "what's in the X **module**?" as often as "what does the X **folder** do?" — especially in Rust where `mod x` backs folder `x/`. Vector search copes via semantic similarity; BM25 and the cross-encoder are exact-token and would miss. ~10 bytes/chunk to add the synonym in the embedded string itself, no query rewriting. Not extending to `package`/`directory` — diluted signal, and `package` collides with `CrateChunk` semantics
35. **Dark-arm pattern — A2 → A3**: A2 shipped FolderChunk infrastructure (table + arm + WASM wiring) with `folder_limit=0` and `folder_vec=false` so nothing leaks into answers. A3 was the single config change that activated routing per intent. Splitting buys harness-signal isolation (A2 alone proves "no regression"; A3 alone proves "recall lifts on folder hero queries") and rollback granularity (A3 is config-rollback-able with A2 infrastructure intact, no re-ingest)
36. **Stratified relationship retrieval — A4**: `folder_vec=false` for Relationship (folder of X displaces consumers of X) but `file_vec=true` (file-level import-graph answers "which files depend on X" — same SOTA pattern as Sourcegraph Cody / Aider / RepoCoder / CodePlan). Granularity matters: same-granularity arms compete usefully on Relationship; coarser-than-target arms displace
37. **Cross-type rerank displacement → introduce `recall@pool` — A4**: A4's first calibration run at `file_limit=3/2/2/2` regressed aggregate r@5 by -9.6pp. Per-arm limits cap pool size, but `RetrievalResult::flatten()` sorts all types by cross-encoder sigmoid — a file chunk scoring 0.63 outranks a code chunk at 0.58 regardless of which type's limit allowed them in. File chunks' "answer-shaped" templates outranked raw code on most queries. Dropped to `2/1/1/1`; only +0.3pp aggregate change. Diagnosed: limit isn't the binding constraint, rerank score order is. Introduced `recall@pool` (recall over every chunk in `RetrievalResult` — all that flow to `build_context` and reach the LLM, no top-k truncation) as a more faithful proxy for RAG pipeline quality than top-k recall under cross-type rerank
38. **Context-section ordering — coarse → granular, code + README query-adjacent**: A2 shipped `crate → folder → file → module_doc → code → readme`. A3 reviewed flipping to granular-first and rejected after research: Lost-in-the-Middle (Liu et al., TACL 2024, arXiv:2307.03172) is U-shaped — primacy + recency both win, middle drops up to 20pp. A2's order gives primacy to architecture framing and recency (query-adjacent slot) to code + README. LongLLMLingua (ACL 2024, arXiv:2310.06839) corroborates query-adjacent privilege. Production systems (Aider repo-map, Sourcegraph Cody Context Fetchers) also ship coarse-first
39. **No new `ExportFolderChunk`/`ExportFileChunk` wrappers — A2/A4**: A2.md drafts proposed dedicated export types. The existing pattern is the generic `EmbeddedChunk<T>` (used for code/readme/crate/module_doc); `EmbeddedChunk<FolderChunk>` and `EmbeddedChunk<FileChunk>` get the same treatment for free. Same applied to chunk IDs — reused `deterministic_chunk_id(file_path, content)` since folder/file path strings already domain-separate from each other
40. **Track A capstone (A5 / RepoSummaryChunk) retired by measurement — 2026-04-18**: drafted as Track A's closer (per-repo manifest summaries with tech stack + entry points + top-level folders). Pre-implementation measurement against 3 hero queries (`a5-main-components`, `a5-how-to-run`, `a5-repo-comparison`) showed recall@10=1.0 across all three using existing chunks today: ReadmeChunks for prose, project-root FolderChunks for top-level structure (the A2 template's `Subfolders:` line already enumerates exactly the data A5 would have carried), CrateChunks for Rust deps. The one r@5 miss (`a5-main-components`) was a lexical collision (`folder:components` ranking #1 on the literal word "components"), not a fundamental gap. A5 retired, infrastructure budget reallocated. The 3 measurement queries kept in the dataset as regression tests

## Quality Harness (V3)

### Structural Foundation

V3 required a structural refactor: module declarations moved from `main.rs` to `src/lib.rs`, enabling a second binary target (`code-rag-harness`) to share library code. `FlatChunk` + `RetrievalResult::flatten()` centralize chunk flattening — used by both API (`build_sources()`) and harness evaluation. Single modification point when new chunk types are added.

### Test Dataset (V3.1 → B4/B5)

Grew from 43 → 101 → 81 (cleaned) declarative test cases with typed expectations. B4 added 48 held-out classifier cases (incl. 3 adversarial Comparison-trap cases); B5 cleanup removed 20 cases targeting non-ingested file types / non-existent entities and added file/identifier targets to the previously classifier-only B4 cases. Net: 73 of 81 cases now score recall (90%); 8 intentionally use only `min_relevant_results` or are unscoreable smoke/edge cases. Three-tier strategy:

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

### Baseline (V3.3 → A4)

**Dual-run mode:** Full pipeline (real classifier) vs. ground-truth intent (bypassed classifier) isolates classifier-induced recall loss. Current numbers measured on the 87-case test dataset (79 recall-scoreable) with composite per-intent `ArmPolicy` + Track A hierarchy arms active (commit df49136, fresh db, label `post_a4_fresh`):

| Metric | Classifier |
|--------|:---------:|
| recall@5 (aggregate) | 0.72 |
| recall@10 (aggregate) | 0.79 |
| **recall@pool** (aggregate) | **0.80** |
| MRR | 0.73 |
| Intent accuracy (97-case held-out corpus) | 74% |

Per-intent recall@5: overview 0.82, implementation 0.74, relationship 0.59, comparison 0.69. Per-intent recall@pool: overview 0.90, implementation 0.79, relationship 0.68, comparison 0.79. Track C closed most of the relationship gap (graph augmentation in C1 + result protection in C2: 0.50 → 0.60) and lifted comparison via per-comparator decomposition (C3: 0.62 → 0.65). Track A added the hierarchy rungs (A2 FolderChunk dark, A3 routing flip — comparison 0.31→0.67 +36pp under the fresh routing baseline; A4 FileChunk — comparison r@pool +12.5pp) and consolidated text primitives into one wasm-pure module (A1). Two stubborn comparison failures remain (`comp-retriever-generator`, `b4-comp-retriever-api`) that share a root cause — BGE-small produces noisy vectors for bare hyphenated identifiers; gated on a future embedder upgrade. Post-B4 the classifier→GT recall gap is ~2pp — classification is no longer the dominant bottleneck. Report metadata (`label`, `completed_tracks`, `hybrid_enabled`, `rerank_enabled`, `dual_embedding_enabled`, `folder_limit_by_intent`, `file_limit_by_intent`) enables comparison across parallel Track improvements and ArmPolicy sweeps.

## Intent-Aware Retrieval

Per-intent `RetrievalConfig` limits across all 6 chunk types (post-A4):

| Intent | code | folder | file | readme | crate | module_doc | bm25 | rerank | folder_vec | file_vec | sig_vec |
|--------|:----:|:------:|:----:|:------:|:-----:|:----------:|:----:|:------:|:----------:|:--------:|:-------:|
| Overview | 5 | 4 | 2 | 3 | 3 | 3 | ✓ | ✓ | ✓ | ✓ | ✗ |
| Implementation | 5 | 1 | 1 | 1 | 1 | 2 | ✗ | ✓ | ✓ | ✓ | ✗ |
| Relationship | 5 | **0** | 1 | 1 | 2 | 2 | ✓ | ✓ | ✗ | ✓ | ✗ |
| Comparison | 5 | 2 | 1 | 2 | 3 | 2 | ✗ | ✗ | ✓ | ✓ | ✗ |

`ArmPolicy` (right 5 columns) was derived empirically — `bm25/rerank/sig_vec` from the B5 8-config × per-intent space sweep, `folder_vec/file_vec` from A3/A4 calibration:

- **Relationship `folder_vec=false`** (A3): first run with `folder_vec=true` for all four intents dropped Relationship 0.60 → 0.55. Failure-trace showed "What uses X?" queries retrieving folder chunks of X *itself*, displacing the actual consumer code in other crates. Consumer discovery is a structural/graph problem and C2's graph-reserve protection covers code chunks, not folders. Gating folder off restored 0.61. The arm is one config flip away if a future relationship hero demonstrates folder value.
- **Relationship `file_vec=true`** (A4, flipped after SOTA review): A4's draft mirrored A3's Relationship gate. SOTA on code RAG (Sourcegraph Cody repo-map, Aider, RepoCoder, CodePlan) uses *stratified* relationship retrieval — function-level call-graph for "what calls X", file-level import-graph for "which files depend on X". A3's folder gate was a mismatched-granularity symptom, not evidence against same-granularity. Empirically: `a4-depends-on-fastembed` (file-level Relationship hero) passes at recall=1.0; consumer-discovery queries take -4pp r@5 but are flat on r@pool — bottleneck is code/graph, not displacement.

Three Track-C / Track-A query-side mechanisms compose on top of the policy without changing the gate values:

- **Graph augmentation** fires on **Relationship + Implementation** intents only (44% Relationship classification accuracy means most relationship queries arrive misclassified as Implementation). Routing vs soft-reserve is selected per-query by `detect_direction`.
- **Comparison decomposition** fires on **Comparison** intent only, conditional on `extract_comparators(query).len() >= 2`. Extraction failure falls through to the unchanged single-arm Comparison path.
- **Hierarchy arms** (folder, file) are RRF-fused alongside the existing arms — the fusion code in `code-rag-engine::fusion` is N-ary and was extended with one new input each, no algorithm change.

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
