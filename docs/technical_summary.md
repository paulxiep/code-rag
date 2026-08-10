# Portfolio RAG Chat — Technical Summary

## Architecture

```
┌──────────────────────────────────────────────────────────────────────────┐
│                             Cargo Workspace                              │
├──────────────────────────────────────────────────────────────────────────┤
│                                                                          │
│  Ingestion-time (native)          Query-time entry points                │
│  ┌──────────────────┐   ┌───────────────┐ ┌──────────────┐ ┌──────────┐  │
│  │ code-rag-ingest  │   │ code-rag-chat │ │ code-rag-mcp │ │code-rag- │  │
│  │ tree-sitter →    │   │ Axum server + │ │ stdio MCP,   │ │ui (Leptos│  │
│  │ chunks + call/   │   │ harness bins  │ │ 9 tools      │ │WASM: chat│  │
│  │ graph edges      │   └──────┬────────┘ └──────┬───────┘ │+ topology│  │
│  └───────┬──────────┘          │                 │         │  tabs)   │  │
│          │ invokes post-ingest └───────┬─────────┘         └────┬─────┘  │
│          ▼                             ▼                        │        │
│  ┌──────────────────┐        ┌──────────────────┐               │        │
│  │   code-raptor    │        │  code-rag-core   │               │        │
│  │ topology engine: │        │ AppState,        │               │        │
│  │ Louvain, drift,  │        │ retrieve(), DTOs │               │        │
│  │ analytics, report│        └────────┬─────────┘               │        │
│  │ + viz + GraphML  │                 │                         │        │
│  └───────┬──────────┘        ┌────────┴────────┐                │        │
│          │                   ▼                 ▼                │        │
│          │          ┌────────────────┐  ┌──────────────┐        │        │
│          └─────────▶│ code-rag-store │  │ code-rag-llm │        │        │
│                     │ LanceDB: 7 vec │  │ Gemini via   │        │        │
│                     │ + 3 scalar     │  │ rig-core     │        │        │
│                     │ tables;        │  └──────────────┘        │        │
│                     │ Embedder +     │                          │        │
│                     │ Reranker       │                          │        │
│                     └───────┬────────┘                          │        │
│                             │                                   │        │
│          ┌──────────────────┴───────────┐                       │        │
│          ▼                              ▼                       ▼        │
│  ┌────────────────┐          ┌───────────────────────────────────────┐   │
│  │ code-rag-types │◄─────────│ code-rag-engine (pure, native+wasm32) │   │
│  │ chunks, edges, │          │ intent · context · fusion · graph ·   │   │
│  │ communities    │          │ comparison · text · folder/file/      │   │
│  └────────────────┘          │ cluster · centrality · mermaid        │   │
│                              └───────────────────────────────────────┘   │
│                                                                          │
│  Store/LLM boundaries are #[wagon] caravan-rpc seams (Embedder /         │
│  Reranker / VectorReader / LlmClient) — one yaml re-projects the same    │
│  binaries onto split deployment topologies with zero source edits.       │
└──────────────────────────────────────────────────────────────────────────┘
```

## Crate Responsibilities

| Crate | Purpose | Key Files |
|-------|---------|-----------|
| `code-rag-ingest` | Ingestion CLI — tree-sitter parsing, language handlers (Rust/Python/TypeScript/Go), incremental ingestion, call + typed relation edge resolution, data export, project purge | `ingestion/`, `edge_resolution.rs`, `import_match.rs`, `export.rs`, `orchestrate.rs`, `main.rs` |
| `code-raptor` | Topology engine (Track R) — builds the relation topology from persisted edges, deterministic Louvain communities + cohesion, ClusterChunk summaries, structural analytics, emergent-vs-folder drift, architecture report + viz JSON + GraphML writers, MCP-facing `insights` facade | `topology.rs`, `louvain.rs`, `cluster.rs`, `clusterchunk.rs`, `betweenness.rs`, `cycles.rs`, `analytics.rs`, `drift.rs`, `report.rs`, `graph_model.rs`, `viz.rs`, `graphml.rs`, `insights.rs` |
| `code-rag-engine` | Shared algorithms — intent classification, context building, scoring, fusion, graph traversal + path tracing, degree centrality, Mermaid rendering, hierarchy/cluster summary templates, single-source text primitives (pure, no I/O, compiles to wasm32) | `intent.rs`, `context.rs`, `retriever.rs`, `fusion.rs`, `graph.rs`, `centrality.rs`, `mermaid.rs`, `comparison.rs`, `text/` (A1), `folder.rs` (A2), `file.rs` (A4), `cluster.rs` (R3) |
| `code-rag-store` | Embedder (FastEmbed) + Reranker + VectorStore (LanceDB, 7 vector + 3 scalar tables) with scored search API; `#[wagon]` Caravan seams (`Embedder` / `Reranker` / `VectorReader` / `VectorWriter`) | `embedder.rs`, `reranker.rs`, `vector_store.rs`, `seams.rs` |
| `code-rag-types` | Shared types — chunks, `CallEdge`, `GraphEdge` (typed relations), `CommunityAssignment`, `ClusterChunk`, deterministic IDs, content hashes | `lib.rs` |
| `code-rag-core` | Chat-side core shared by the chat binary and MCP — `AppState`, `retrieve()`, DTOs (extracted at Caravan M5) | `state.rs`, `retriever.rs` |
| `code-rag-llm` | LLM-provider seam impls (rig-core-backed Gemini) | `lib.rs` |
| `code-rag-chat` | Query API — HTTP routing, LLM generation, quality harness, serves WASM UI | `api/`, `harness/`, `bin/harness.rs` |
| `code-rag-mcp` | MCP stdio server exposing the retrieval brain to Claude Code as nine tools (five retrieval + four topology); ships the [Claude Code skill](../crates/code-rag-mcp/skills/code-rag.md) that routes queries between Grep/Read and the MCP tools | `src/main.rs`, `src/topology_tools.rs`, `skills/code-rag.md` |
| `code-rag-ui` | Leptos WASM SPA — chat + topology tabs (default: backend API, standalone: in-browser RAG + d3-force community graph) | `components/`, `standalone_api.rs`, `viz_data.rs`, `graph_bridge.rs`, `static/graph.js` |

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

## Storage Schema (7 Vector Tables + 3 Scalar Tables)

| Table | Content | Embedding Input | FTS / BM25 Target |
|-------|---------|-----------------|-------------------|
| `code_chunks` | Functions, classes, structs, traits, enums, interfaces | `identifier + docstring + code + calls` (body_vector) + `signature + language + docstring` (nullable `signature_vector`, shipped OFF) | `searchable_text` = identifier (2×) + camelCase split + signature + docstring |
| `folder_chunks` (A2) | One row per directory | `summary_text` from `code_rag_engine::folder::render_summary` — 5-line template: `Folder: path (module: basename) / Contains: N files (langs) / Key types: ... / Key functions: ... / Subfolders: ...` | `summary_text` |
| `file_chunks` (A4) | One row per source file | `summary_text` from `code_rag_engine::file::render_summary` — 4-line template: `File: path (module: basename, language) / Exports: ... / Imports: ... / Purpose: ...` | `summary_text` |
| `cluster_chunks` (R3) | One row per emergent community | `summary_text` from `code_rag_engine::cluster::render_summary` — deterministic template (size, key types/functions, files, dominant relation, cohesion, likely concern) | `summary_text` (retrieval arm gated OFF by measurement; rows also feed the architecture report + MCP `code_rag_communities`) |
| `readme_chunks` | README.md files | `Project: name + content` | `content` |
| `crate_chunks` | Cargo.toml metadata | `Crate: name + description + dependencies` | natural text |
| `module_doc_chunks` | Module-level docs (`//!`) | `Module: name + doc_content` | natural text |
| `call_edges` | Caller→callee call relationships | (none — scalar-only) | (none) |
| `graph_edges` (R1) | Typed relation edges: imports / re_exports / contains / implements / extends / embeds / references / rationale_for, each with context tag + confidence (extracted/inferred) | (none — scalar-only) | (none) |
| `community_assignments` (R2) | Per-code-chunk community id + cohesion from deterministic Louvain | (none — scalar-only) | (none) |

`folder_chunks` / `file_chunks` Arrow schema uses **native `List<Utf8>`** for vec metadata fields (languages / key_types / key_functions / subfolders / exports / imports), matching the post-V1.1 `crate_chunks.dependencies` pattern — no JSON-encoded blobs. `summary_text` is persisted on the row (not re-rendered) so server-embedded bytes and browser BM25 bytes can never drift; the pure render function in `code-rag-engine` is the single source of truth.

`call_edges` schema: deterministic `edge_id`, caller/callee `chunk_id` + identifier + file, `project_name`, `resolution_tier: u8` (1=same_file, 2=import_based, 3=unique_global). Validated against LanceDB with a dedicated integration test before building the API. Queried directly by the graph traversal helpers in `code-rag-engine::graph` — no vector search.

## Ingestion Pipeline

```
Source Files (.rs, .py, .ts, .tsx, .js, .jsx)
    │
    ▼
┌─────────────────┐
│  LanguageHandler │  Trait-based: RustHandler, PythonHandler, TypeScriptHandler, GoHandler
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
┌─────────────────┐
│ Edge Resolution │  call edges (3-tier: same-file → import-anchored → unique-in-
│                 │  project) + typed relation edges (imports/type relations/
│                 │  containment/rationale) — identifier index keyed (project,
│                 │  identifier): the graph never links sibling projects
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│ Topology Stage  │  code_raptor::build_topology (per project) — communities,
│  (code-raptor)  │  ClusterChunks, analytics, report + viz JSON + GraphML
└────────┬────────┘
         │
         ▼
    LanceDB (7 vector + 3 scalar tables) + data/reports/ + data/viz/
```

## Topology Stage (Track R — Code Raptor)

Runs after every ingest and as a standalone cluster-only re-run
(`code-rag-ingest topology [--project-name] [--report-dir] [--viz-dir]`) — no
re-parsing. SoC: ingestion *writes* edges, topology *reads* them
(`code-raptor` depends on store + types + engine, never on `code-rag-ingest`).

```
call_edges + graph_edges (persisted)
    │
    ▼
Topology::build        one weighted undirected graph per project.
                       Kept: calls ∪ imports/re_exports ∪ implements/extends/
                       embeds/references ∪ file→function contains.
                       Dropped: folder→file contains — feeding the folder tree
                       into clustering would make communities recover the
                       folders and make the drift comparison self-fulfilling.
    │
    ▼
cluster::detect        deterministic Louvain (fixed node order, sorted
                       neighbor iteration, no RNG) + utility-hub exclusion with
                       majority-vote reattach + oversized/low-cohesion
                       re-splits + per-community cohesion. Identical partition
                       and community ids on every run.
    │
    ├──▶ community_assignments (additive scalar table, per code chunk)
    ├──▶ cluster_chunks (template summaries, embedded — retrieval arm gated OFF)
    │
    ▼
analytics::compute     derived per run, never stored: degree centrality
+ drift::compare       (wasm-safe, shared with the browser), Brandes
                       edge-betweenness bridges (+ surprise ranking, relation
                       provenance per bridge), import cycles (Tarjan SCC +
                       bounded canonical DFS), emergent-vs-folder drift
                       (community purity vs directory layout)
    │
    ▼
Artifacts              architecture_<project>.md (report incl. drift section),
                       graph_viz_<project>.json (browser artifact, capped
                       5000 nodes / 15 000 edges), topology_<project>.graphml
                       (full graph, Gephi/yEd) — all byte-deterministic
                       (run-twice tested; two full runs produce identical bytes)
```

The MCP topology tools do **not** re-run any of the heavy algorithms: they
read persisted `community_assignments` / `cluster_chunks` and recompute only
O(V+E) analytics (degree, cycles, drift) through the `code_raptor::insights`
facade, so tool output always matches the report and stays sub-second.

## Docstring Extraction

| Language | Strategy | Patterns |
|----------|----------|----------|
| Rust | Scan backwards from node | `///` outer doc, `#[doc = "..."]` attribute form |
| Python | AST traversal into body | `"""..."""` / `'''...'''` first expression_statement |
| TypeScript | Scan backwards for JSDoc | `/** ... */`, filters out `@param`/`@returns` |
| Go | Scan backwards for `//` block | contiguous `//` lines, blank-line breaks association, `//go:` directives skipped |

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
41. **Deterministic Louvain, hand-rolled — R2**: community detection uses a from-scratch Louvain (modularity maximization) with fixed node visiting order, sorted neighbor iteration, smallest-community tie-breaks, and size-desc/min-chunk-id re-indexing — no RNG anywhere, so identical input produces identical communities and ids across runs. Rust has no graspologic/Leiden equivalent and petgraph ships no community detection, so it was from-scratch either way; the declared petgraph dep was dropped in R4 (never used — Louvain, Brandes, Tarjan and the cycle DFS are all hand-rolled for determinism control). Leiden deferred as an optional refinement; revisit trigger is an internally-disconnected community appearing in the report
42. **Folder edges excluded from partition input — R2**: the partition runs over calls ∪ imports ∪ type relations ∪ **file→function** containment only. Folder→file `contains` edges are persisted but never fed to clustering — high-level folders are frequently not cohesive, and including the folder tree would make communities recover the folders, rendering the R5 emergent-vs-folder drift comparison self-fulfilling. File nodes stay in the graph as import-signal connectors, flagged as containers and excluded from persistence
43. **Empirical gating of the cluster retrieval arm — R3**: ClusterChunk summaries shipped as a full retrieval arm, then a ground-truth per-intent sweep showed they *displace* the code/folder chunks that already answer architecture queries (Overview recall@5 −4pp at every limit). The arm is gated OFF (`cluster_limit=0`) with all machinery wired — same pattern as B2 hybrid (shipped → disabled → re-enabled once the root cause was fixed). The rows still power the architecture report and the MCP communities tool, so the work pays for itself outside retrieval
44. **Analytics derived, never stored — R4**: centrality, bridges, surprise ranking, cycles and drift are recomputed from the persisted edge tables on every run rather than persisted. No retrieval consumer exists; the R5 MCP tools stay sub-second anyway because they never run the heavy algorithms (Louvain/betweenness) — they read persisted `community_assignments` and recompute only O(V+E) analytics via the `insights` facade, guaranteeing tool output ≡ report ≡ DB
45. **Tarjan SCC + bounded canonical DFS over Johnson's — R4**: R.md nominally specified Johnson (1975) for cycle enumeration, but Johnson's unblocking logic assumes complete exploration — under a cycle-length cap (12, max 50 cycles) a depth-pruned blocked search can *miss short cycles*. SCC restriction + min-vertex-canonical bounded DFS is exact for every cycle within the cap and cheap on mostly-acyclic import graphs
46. **serde for machine formats, hand-rolled for markup — R5**: the viz JSON is serialized with serde_json (struct-declaration field order + Vec-only shapes ⇒ byte-deterministic; JSON-escaping user paths by hand is where bugs live), while markdown (report), XML (GraphML) and Mermaid remain hand-rolled string writers — fixed document shapes where an escaping helper is 5 lines and byte-determinism stays trivially auditable. All artifacts carry run-twice byte-identity tests
47. **Explicit params over NL parsing for path queries — R5**: `detect_direction` recognizes "path between / flow / trace" phrasings but deliberately does **not** attempt to parse two identifiers out of natural language (fragile). Two-endpoint queries are served by the explicit-params route — `graph::path_augment(from, to)` (which finally constructs `GraphDirection::Path` in production) behind the MCP `code_rag_path` tool, with unknown-vs-ambiguous identifier resolution distinguished for clean error messages
48. **One MCP-facing facade — R5**: `code-raptor`'s modules stay private; `pub mod insights` is the single deliberate public surface (central nodes, drift, cycles re-exports). MCP gains a direct `code-raptor` dependency (legal direction: raptor depends only on store + types + engine), and response shaping lives in a pure, unit-testable `topology_tools` module rather than the rmcp handlers
49. **Browser topology as precomputed artifact — R5**: the demo never runs graph algorithms beyond force layout. The native side emits a capped per-project `graph_viz_<project>.json` (top-5000 nodes by degree, 15 000 edges, low-value inferred-references edges dropped first, pre-cap totals recorded); the UI lazy-loads it on first tab open (keeping the ~33 MB `index.json` first-paint path untouched), hands the raw JSON to a `static/graph.js` d3-force singleton via the established global-function bridge, and reads its palette from `--viz-*` CSS custom properties so the graph re-skins live on theme flips. 8 fixed community color slots + a neutral "Other" — hues never wrap, so the legend never lies

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

**Dual-run mode:** Full pipeline (real classifier) vs. ground-truth intent (bypassed classifier) isolates classifier-induced recall loss. Current numbers measured with composite per-intent `ArmPolicy`, hybrid + rerank, classifier routing on the 6-project corpus (commit 8064e31, label `post_rationale_anchor`):

| Metric | Classifier |
|--------|:---------:|
| recall@5 (aggregate) | 0.60 |
| recall@10 (aggregate) | 0.69 |
| **recall@pool** (aggregate) | **0.72** |
| Intent accuracy (97-case held-out corpus) | 74% |

Per-intent recall@5 / @10 / @pool: overview 0.70/0.77/0.81, implementation 0.61/0.67/0.67, relationship 0.47/0.61/0.61, comparison 0.62/0.73/0.75.

**Corpus note — not comparable to the earlier post-A4 0.72@5**: two portfolio projects were purged (source repos deleted from disk), the R1 cross-project resolution leak was fixed (edges that once resolved into other projects inflated recall with wrong answers), and the corpus drifted for months against a frozen test dataset (concurrens grew, code-rag gained its own Track R code). The 2026-08 leak fix itself moved per-intent numbers only within noise. Track history on the old corpus: Track C closed most of the relationship gap (C1+C2: 0.50 → 0.60) and lifted comparison via decomposition (C3); Track A added the hierarchy rungs (A3 comparison +36pp) and introduced `recall@pool`; Track R is retrieval-neutral by design (its one arm, R3 clusters, was measured net-negative and gated OFF). Two stubborn comparison failures remain (`comp-retriever-generator`, `b4-comp-retriever-api`) — BGE-small produces noisy vectors for bare hyphenated identifiers; gated on a future embedder upgrade. Post-B4 the classifier→GT recall gap is ~2pp — classification is no longer the dominant bottleneck. Report metadata (`label`, `completed_tracks`, arm/limit flags) enables comparison across parallel Track improvements and ArmPolicy sweeps.

## Intent-Aware Retrieval

Per-intent `RetrievalConfig` limits (set at A4, still current — R3 added a seventh chunk type, `cluster`, whose arm is gated to 0 on every intent by measurement):

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

## MCP Server (Claude Code integration)

The `code-rag-mcp` crate exposes the retrieval brain as a Model Context Protocol stdio server. Nine tools — five retrieval + four topology (R5) — all read-only against the LanceDB index except `reindex`; no LLM calls (Claude Code is the LLM, this is the retriever).

| Tool | Engine seam wrapped | Notes |
|---|---|---|
| `code_rag_search(query, intent?)` | `code_rag_core::retriever::retrieve` (extracted from the chat binary at Caravan M5) | Full no-LLM pipeline — embed → classify (or accept hint) → route → vector + BM25 + rerank + RRF + graph augment + comparison decomposition. Returns ranked chunks shaped via `code_rag_core::build_sources` with the `chunk_id` field. |
| `code_rag_graph(identifier, direction?)` | Loads all `CallEdge`s via `VectorStore::get_all_edges`, builds an in-memory `code-rag-engine::graph::CallGraph`, resolves identifier → chunk_id via `unique_chunk_for_identifier`, then queries `VectorStore::get_callers` / `get_callees` for the metadata-rich edge views. | Bypasses vector search entirely — pure graph query. Cross-project resolution works in workspace mode. |
| `code_rag_overview(topic?)` | Same `retrieve` path as `code_rag_search`, with `intent` forced to `QueryIntent::Overview`. | Surfaces README / crate / folder / module-doc / file-summary chunks ahead of code. |
| `code_rag_neighbors(chunk_id, window?)` | `VectorStore::get_chunks_by_ids` followed by a line-windowed file read. | Resolves code chunks only today; non-code chunk_ids return "not found". Path resolution falls back from direct join to first-component-stripped to handle both `--single-repo` and multi-project layouts. |
| `code_rag_reindex(mode?)` | Subprocess: spawns `std::env::current_exe()` with the internal `ingest` subcommand (which calls `code_rag_ingest::ingest_repo`). Adds `--single-repo` unless `--workspace` was passed at MCP startup; adds `--full` only when `mode: "full"` (default `incremental`). Blocks until exit. | Subprocess isolation means an ingest panic doesn't take down the search-serving process. LanceDB tables are opened per-query in `VectorStore`, so no explicit reload is needed after the subprocess exits. The topology stage (communities + report + viz artifacts) runs inline at the end of every ingest. |
| `code_rag_communities(project?)` | Seam reads: `get_cluster_chunks` + `get_community_assignments` + `get_chunks_by_ids`; drift via `code_raptor::insights::drift`. | Emergent modules (size, cohesion, key functions/types, home directory) + the drift object (mean purity, scattered communities, fragmented folders). "What are the main subsystems / does the folder structure match reality?" |
| `code_rag_central_nodes(project?, limit?)` | Seam reads: edge tables + assignments + chunks; `code_raptor::insights::central_nodes` (weighted-degree ranking, containers + cross-project chunks filtered). | The "read these first" list. Community ids attached from the persisted partition. Default limit 10, cap 50. |
| `code_rag_cycles(project?)` | Seam read: `get_all_graph_edges`; `code_raptor::insights::find_import_cycles` (Tarjan SCC + bounded canonical DFS). | File-level import cycles. Empty result is reported as a positive signal (acyclic import graph). |
| `code_rag_path(from, to)` | In-memory `CallGraph` + `code_rag_engine::graph::path_augment` (forward, then reverse); hop labels via `get_chunks_by_ids`; diagram via `code_rag_engine::mermaid::render_call_path`. | Shortest call chain between two identifiers, each hop as identifier + file + chunk_id, plus a Mermaid `flowchart LR` snippet. Unknown/ambiguous identifiers produce named invalid-params errors. |

**SDK and tool registration.** Built on `rmcp` 1.5 (the official Rust SDK from the MCP org). Tool handlers are plain `async fn`s on `CodeRagServer` annotated with `#[tool(description = "...")]` inside a `#[tool_router]` impl block. `#[tool_handler]` on `impl ServerHandler` wires the router to the rmcp dispatch loop. Parameters arrive via `Parameters<T>` wrappers where `T: Deserialize + JsonSchema`; rmcp generates the JSON Schema advertised in `tools/list` from the schemars derivation.

**Process model.** Single-request-at-a-time inside the search path. The embedder and reranker live in `Mutex<...>` on the existing `AppState` (reused from `code-rag-chat::api::state::AppState` to avoid duplicating 746 lines of native orchestration). Stdout is the JSON-RPC channel — `tracing_subscriber::fmt().with_writer(std::io::stderr)` is load-bearing; any debug output to stdout corrupts the protocol stream.

**chunk_id plumbing.** A `chunk_id: String` field exists on every chunk type in `code-rag-types` (deterministic SHA-based via `deterministic_chunk_id`) but was originally dropped during `RetrievalResult::flatten()`. The MCP work added it back to `FlatChunk`, propagated through both `SourceInfo`s (server `src/api/dto.rs` and WASM `crates/code-rag-ui/src/api.rs`), and exposed it on every search result so `code_rag_neighbors` can refetch a specific chunk by id without re-querying.

**Two ingestion layouts.** The MCP works against either:
- A `--single-repo` index (one project, repo-relative paths) — the standalone external-user case.
- A multi-project parent-dir index (the portfolio's existing layout, paths prefixed by project name) — opt-in via `--workspace`.

`code_rag_neighbors` handles both layouts transparently: it tries the direct path under `--repo-path` first, then on `ENOENT` retries with the first path component stripped.

**Distribution.** Single binary, three-step install, no terminal commands once the exe is on disk.

- The release zip ([cut by .github/workflows/release.yml](../.github/workflows/release.yml), `workflow_dispatch`-triggered, matrix-built for `x86_64-unknown-linux-gnu` / `aarch64-apple-darwin` / `x86_64-pc-windows-msvc`) ships exactly one binary, `code-rag-mcp`, plus a `code-rag-mcp.config.yaml` template and a `README.txt`.
- The binary has three behaviours, dispatched by argv:
  - **Bare run** (`code-rag-mcp` with no args, e.g. double-click) — reads `code-rag-mcp.config.yaml` from `current_exe().parent()`, parses `target_path` + `workspace`, then writes `.claude/skills/code-rag.md` (skill embedded via `include_str!("../skills/code-rag.md")` at compile time), merges a `code-rag` entry into `.mcp.json`'s `mcpServers` (preserving any existing servers via `serde_json::Value` round-trip), and appends `.code-rag-mcp/` to `.gitignore` (idempotent). On first run with no config present, writes the template there and exits with instructions.
  - **Serve** (top-level flags like `--db-path`, `--repo-path`, `--workspace`) — runs the rmcp stdio server. This is what Claude Code spawns via `.mcp.json`.
  - **`ingest <path>` subcommand** — calls `code_rag_ingest::ingest_repo(IngestOpts { … })`, the lib entrypoint in [crates/code-rag-ingest/src/orchestrate.rs](../crates/code-rag-ingest/src/orchestrate.rs). Used by `code_rag_reindex` to spawn `std::env::current_exe()` recursively, which is why the release ships only one binary instead of two. The post-ingest topology stage (`code_raptor::build_topology`) runs inline, so MCP users get the architecture report + viz/GraphML artifacts under `.code-rag-mcp/{reports,viz}/` on every ingest.
- The standalone `code-rag-ingest` CLI still exists as a workspace crate for the portfolio ingest + GitHub Pages export but is not in the user-facing release.

End-user install is: download zip → extract → edit `target_path` in the YAML → run the exe. Open Claude Code in the target dir; the bundled skill instructs the agent to call `code_rag_reindex mode=full` for the initial ingest. Ship details: [docs/release.md](release.md).

## Build & Run

```bash
# Ingest repositories
docker-compose -f docker-compose-ingest.yaml up

# Run query server (Docker)
docker-compose up

# Export data for static demo
cargo run -p code-rag-ingest -- export --db-path data/portfolio.lance --output crates/code-rag-ui/static/index.json

# Rebuild the topology only (communities + report + viz/GraphML artifacts, no re-parse)
cargo run -p code-rag-ingest -- topology --db-path data/portfolio.lance

# Build static GitHub Pages demo
trunk build --release --features standalone crates/code-rag-ui/index.html

# Run quality harness (dual-run baseline)
cargo run --release --bin code-rag-harness -- --verbose
cargo run --release --bin code-rag-harness -- --ground-truth-intent --label baseline_gt --verbose
```
