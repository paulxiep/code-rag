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

## Vector Schema (4 Vector Tables + 1 Scalar Table)

| Table | Content | Embedding Input | FTS / BM25 Target |
|-------|---------|-----------------|-------------------|
| `code_chunks` | Functions, classes, structs, traits, enums, interfaces | `identifier + docstring + code + calls` (body_vector) + `signature + language + docstring` (nullable `signature_vector`, shipped OFF) | `searchable_text` = identifier (2×) + camelCase split + signature + docstring |
| `readme_chunks` | README.md files | `Project: name + content` | `content` |
| `crate_chunks` | Cargo.toml metadata | `Crate: name + description + dependencies` | natural text |
| `module_doc_chunks` | Module-level docs (`//!`) | `Module: name + doc_content` | natural text |
| `call_edges` | Caller→callee call relationships | (none — first scalar-only LanceDB table) | (none) |

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
    LanceDB (4 vector tables + call_edges)
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

### Baseline (V3.3 → C3)

**Dual-run mode:** Full pipeline (real classifier) vs. ground-truth intent (bypassed classifier) isolates classifier-induced recall loss. Current numbers measured on the 81-case cleaned dataset with composite per-intent `ArmPolicy` + Track C (commit ee22398, label `c3_post8`):

| Metric | Classifier |
|--------|:---------:|
| recall@5 (aggregate) | 0.72 |
| recall@10 (aggregate) | 0.76 |
| MRR | 0.71 |
| Intent accuracy (97-case held-out corpus) | 74% |

Per-intent recall@5: overview 0.80, implementation 0.76, relationship 0.60, comparison 0.65. Track C closed most of the relationship gap (graph augmentation in C1 + result protection in C2: 0.50 → 0.60) and lifted comparison via per-comparator decomposition (C3: 0.62 → 0.65). Two stubborn comparison failures remain (`comp-retriever-generator`, `b4-comp-retriever-api`) that share a root cause — BGE-small produces noisy vectors for bare hyphenated identifiers; gated on a future embedder upgrade. Post-B4 the classifier→GT recall gap is ~2pp — classification is no longer the dominant bottleneck. Report metadata (`label`, `completed_tracks`, `hybrid_enabled`, `rerank_enabled`, `dual_embedding_enabled`) enables comparison across parallel Track improvements and ArmPolicy sweeps.

## Intent-Aware Retrieval

| Intent | code | readme | crate | module_doc | bm25 | rerank | sig_vec |
|--------|:----:|:------:|:-----:|:----------:|:----:|:------:|:-------:|
| Overview | 5 | 3 | 3 | 3 | ✓ | ✓ | ✗ |
| Implementation | 5 | 1 | 1 | 2 | ✗ | ✓ | ✗ |
| Relationship | 5 | 1 | 2 | 2 | ✓ | ✓ | ✗ |
| Comparison | 5 | 2 | 3 | 2 | ✗ | ✗ | ✗ |

`ArmPolicy` (right 3 columns) was derived empirically from a B5 8-config × per-intent space sweep — each gate is the arg-max of that intent's recall@5 curve.

Two Track-C query-side mechanisms compose on top of the policy without changing the gate values:

- **Graph augmentation** fires on **Relationship + Implementation** intents only (44% Relationship classification accuracy means most relationship queries arrive misclassified as Implementation). Routing vs soft-reserve is selected per-query by `detect_direction`.
- **Comparison decomposition** fires on **Comparison** intent only, conditional on `extract_comparators(query).len() >= 2`. Extraction failure falls through to the unchanged single-arm Comparison path.

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
