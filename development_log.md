# Development Log

## 2026-04-03: V3.3 — Baseline Quality Metrics

### Summary

Ran the V3.2 harness against the V2 index in dual-run mode (full pipeline + ground-truth intent) and committed the first quantitative baseline. Added report metadata (`label`, `completed_tracks`) for tracking across parallel Tracks A/B/C. Changed ground-truth mode to skip cases without `expected_intent` instead of hard-erroring, making the dual-run workflow practical.

### Motivation

- **Quantitative "before":** Every future Track improvement needs a baseline to compare against. V3.3 establishes that baseline with concrete numbers.
- **Classifier vs. retrieval isolation:** Dual-run reveals that ground-truth routing barely improves recall (+0.02), proving retrieval quality — not classification — is the bottleneck for Tracks A/B/C.
- **Per-intent breakdown for Track prioritization:** Overview recall is perfect (1.00), implementation is solid (0.70), relationship is weak (0.38), comparison is good (0.75). This directly informs which Tracks to prioritize.

### Baseline Results

**Full Pipeline (real classifier):**

| Metric | Value |
|--------|-------|
| recall@5 | 0.65 |
| recall@10 | 0.65 |
| MRR | 0.60 |
| Intent accuracy | 62% |
| Latency p50 | 115ms |
| Latency p95 | 204ms |

**Ground-Truth Intent (bypassed classifier):**

| Metric | Value |
|--------|-------|
| recall@5 | 0.67 |
| recall@10 | 0.67 |
| MRR | 0.61 |
| Intent accuracy | 100% |
| Latency p50 | 57ms |
| Latency p95 | 80ms |

**Per-Intent Breakdown (full pipeline):**

| Intent | Queries | recall@5 | Intent Acc |
|--------|---------|----------|-----------|
| overview | 8 | 1.00 | 62% |
| implementation | 15 | 0.70 | 73% |
| comparison | 4 | 0.75 | 50% |
| relationship | 5 | 0.38 | 40% |

### Key Observations

- **Classifier doesn't hurt recall:** Ground-truth routing only improves recall@5 from 0.65 to 0.67 (+0.02). The classifier is wrong 38% of the time but retrieval still finds the right content. Focus on retrieval quality, not classification.
- **Latency halves without classifier:** p50 drops from 115ms to 57ms. The classifier adds ~60ms overhead (embedding comparison against prototypes).
- **Overview retrieval is solved:** recall@5 = 1.00 — README and crate chunks embed well with BGE-small.
- **Relationship queries are weakest:** recall@5 = 0.38, exactly as predicted (0.2–0.5 range). Pure vector search cannot resolve call chains. This is the gap Track C addresses.
- **recall@5 == recall@10:** No additional relevant results appear in positions 6–10. The system either finds it in top-5 or doesn't find it at all.
- **4 never-found files:** `state.rs`, `export.rs`, `languages/mod.rs`, `rust.rs` — these exist in the codebase but never appear in any query's top-K results. Targets for Track B (hybrid search) improvement.

### What Changed

**New files:**
- `data/reports/baseline_51e6de5.json` — Full pipeline baseline (JSON)
- `data/reports/baseline_51e6de5.md` — Full pipeline baseline (Markdown)
- `data/reports/baseline_gt_51e6de5.json` — Ground-truth intent baseline (JSON)
- `data/reports/baseline_gt_51e6de5.md` — Ground-truth intent baseline (Markdown)

**Modified files:**
- `src/harness/report.rs` — Added `label: String` and `completed_tracks: Vec<String>` to `SystemConfig` for tracking across parallel Tracks; added label display in Markdown report header
- `src/bin/harness.rs` — Added `--label` (default: `"baseline"`) and `--track` (repeatable) CLI args; filenames now use `{label}_{hash}` pattern
- `src/harness/runner.rs` — Ground-truth mode now skips cases without `expected_intent` (with verbose warning) instead of hard-erroring; enables dual-run on full dataset
- `v3.3.md` — Refined: added per-intent expectation table with Track mapping, Baseline→Track Handoff section, dual-run process, dataset freeze policy, metadata-based naming convention

### Key Design Decisions

- **Skip vs. hard-error in ground-truth mode:** Changed from hard error to skip-with-warning for cases without `expected_intent`. The original design prevented running ground-truth mode on the full 43-case dataset (11 smoke/edge cases lack intent). Skipping makes the dual-run workflow practical without requiring tag filtering.
- **Metadata in JSON, not filenames:** `label` and `completed_tracks` stored in the report's `system` object. Handles parallel track completion (A1+B1) without combinatorial filename explosion.
- **Baseline against pre-V3 index:** Intentionally did not re-ingest before baseline. V3 only added harness infrastructure — the baseline measures V2 retrieval quality, which is the correct "before" for Track comparisons.
- **Dataset freeze policy:** The 43 test cases committed here are the baseline contract. Future Tracks add new cases but do not modify existing ones, preserving comparison validity.

### Test Results

192 tests pass (0 new tests in V3.3 — operational milestone), 0 failures, 5 ignored (require external resources). Clippy clean with `-D warnings`. Fmt clean.

---

## 2026-04-02: V3.2 — Recall Measurement Harness

### Summary

Built `code-rag-harness`, a second binary that measures retrieval quality by running test queries against the real engine pipeline (embed → classify → route → retrieve), stopping before LLM generation. Produces JSON + Markdown reports with recall@K, MRR, intent accuracy, and latency percentiles. Includes a structural refactor: extracted `src/lib.rs` and added `FlatChunk`/`flatten()` to centralize chunk flattening across harness and API.

### Motivation

- **Quantitative baseline for Tracks A/B/C:** Every future improvement (hierarchy, BM25, call graph) needs a "before" number. The harness produces this baseline.
- **Two evaluation modes:** Full pipeline (real classifier) catches end-to-end regressions. Ground-truth mode (bypasses classifier) isolates pure retrieval quality for A/B comparisons.
- **lib.rs extraction:** Rust requires shared library code for multi-binary crates. This structural correction unlocks all future binary extensions without modifying the library again.

### What Changed

**New files:**
- `src/lib.rs` — Module declarations extracted from main.rs (structural correction for multi-binary crate)
- `src/bin/harness.rs` — CLI entry point with clap (dataset, db-path, output, ground-truth-intent, strict, tag, verbose flags)
- `src/harness/runner.rs` — `QueryResult`, `RetrievedItem` types; `run_all()` async execution against real pipeline; `to_retrieved_items()` flattening with 1-indexed ranks
- `src/harness/matching.rs` — Pure hit detection functions (`matches_file`, `matches_identifier`, `matches_chunk_type`, `matches_project`, `matches_excluded_file`); `HitResult` struct; `evaluate_hits()` for all 7 TestCase expectation fields
- `src/harness/metrics.rs` — `recall_at_k()`, `mrr()`, `percentile()`; `AggregateMetrics` and `IntentMetrics` structs; `compute_aggregate()` and `compute_by_intent()` aggregation
- `src/harness/report.rs` — `HarnessReport`, `SystemConfig`, `QueryReport` structs; JSON + Markdown output; post-run warning generation; `git_short_hash()` helper

**Modified files:**
- `src/main.rs` — `mod` declarations replaced with `use code_rag_chat::*` imports
- `src/engine/mod.rs` — Added re-exports for `RetrievalConfig` and `FlatChunk`
- `src/harness/mod.rs` — Added submodule declarations (runner, matching, metrics, report)
- `src/harness/dataset.rs` — Added `validate_strict()` method (promotes warnings to errors for CI)
- `crates/code-rag-engine/src/retriever.rs` — Added `FlatChunk` struct and `RetrievalResult::flatten()` method (centralized flattening with relevance DESC, file_path ASC sort)
- `crates/code-rag-engine/src/intent.rs` — Added `impl FromStr for QueryIntent` (parses "overview"/"implementation"/"relationship"/"comparison")
- `src/api/dto.rs` — Simplified `build_sources()` to use `flatten()`, removed 4 `from_scored_*` helper methods
- `Cargo.toml` — Added `[[bin]]` entries for both binaries, `clap` and `chrono` dependencies

### Key Design Decisions

- **`FlatChunk` + `flatten()` centralization:** Single source of truth for flattening typed chunk vectors. Used by both API (`build_sources()`) and harness evaluation. When Track A adds `FolderChunk`, only one `flatten()` arm needs updating.
- **Pure matching/metrics modules:** All hit detection and metric computation are pure functions with no I/O — fully unit-testable without embedder, database, or async runtime.
- **Coverage checks separate from recall:** `expected_projects`, `expected_chunk_types`, `min_relevant_results`, and `excluded_files` are boolean checks in `HitResult`, not part of the recall denominator. Recall stays focused on content retrieval (files + identifiers).
- **Warmup embed before measurement:** Prevents embedder model load cost (~50MB) from skewing latency percentiles on small datasets.
- **Ground-truth mode hard error:** Missing `expected_intent` in ground-truth mode fails the run immediately — prevents biased metrics from silent fallback.

### Architecture

```
code-rag-harness binary
  → harness module (dataset, runner, matching, metrics, report)
  → engine module (classify, route, retrieve, FlatChunk, flatten)
  → store module (Embedder, VectorStore)

Does NOT depend on:
  ✗ api module (no HTTP layer)
  ✗ engine::generator (no LLM calls)
```

### Test Results

96 tests pass (41 new + 55 existing), 0 failures, 1 ignored (requires GEMINI_API_KEY). Clippy clean. Fmt clean.

| Module | New Tests |
|--------|-----------|
| `code-rag-engine/retriever.rs` | 5 (flatten sort, tiebreaker, line/no-line, empty) |
| `code-rag-engine/intent.rs` | 2 (FromStr valid variants, invalid) |
| `api/dto.rs` | 6 (refactored: build_sources per chunk type + sort + relevance_pct) |
| `harness/dataset.rs` | 2 (validate_strict good/bad) |
| `harness/runner.rs` | 2 (to_retrieved_items ranking, empty) |
| `harness/matching.rs` | 25 (5 match functions + 20 evaluate_hits scenarios) |
| `harness/metrics.rs` | 11 (recall, MRR, percentile, aggregate) |
| `harness/report.rs` | 3 (JSON round-trip, Markdown render, git hash) |

---

## 2026-04-02: V3.1 — Retrieval Test Dataset

### Summary

Added `TestCase` and `TestDataset` types with a 43-query JSON test corpus (`data/test_queries.json`). This is the first step of the V3 quality harness — a declarative, forward-compatible test dataset that outlives any retrieval strategy change. Tests reference stable identifiers (file paths, function names), not implementation details (chunk IDs, embeddings).

### Motivation

- **Quantitative regression safety:** V1-V2 relied on manual hero queries. Tracks A/B/C will change retrieval behavior — need automated recall measurement to detect regressions.
- **Forward compatibility:** Schema uses `#[serde(default)]` on all optional fields, so future Track fields (`expected_folder_paths`, `expected_bm25_hits`, `expected_callers`) can be added without breaking existing test cases.
- **Three-tier strategy:** Hero queries (strict, all dimensions) anchor regressions. Directional queries (1-2 dimensions) track quality per intent. Smoke queries (`min_relevant_results`/`excluded_files` only) survive any pipeline change.

### What Changed

**New files:**
- `src/harness/mod.rs` — Module root for quality harness infrastructure
- `src/harness/dataset.rs` — `TestCase`, `TestDataset` types with serde derives; `load()`, `filter_by_tag()`, `validate()` methods; 15 unit tests covering serde round-trips, filtering, validation, and edge cases
- `data/test_queries.json` — 43 test cases across 4 intent categories (overview, implementation, relationship, comparison) and 3 tiers (hero, directional, smoke)

**Modified files:**
- `src/main.rs` — Added `mod harness;` declaration
- `crates/code-rag-ui/src/api.rs` — Fixed pre-existing clippy dead_code warning on `SourceInfo.relevance`
- `crates/code-rag-ui/src/components/chat_view.rs` — Fixed pre-existing clippy collapsible_if warning
- `crates/code-raptor/src/export.rs` — Fixed pre-existing clippy collapsible_if warning
- `architecture.md` — Added V3 harness module to code-rag-chat component diagram, `FlatChunk`/`flatten()` + `FromStr` to code-rag-engine diagram, updated crate responsibilities table

### Key Design Decisions

- **Substring matching for files:** `"retriever.rs"` matches `"src/engine/retriever.rs"`. Survives directory restructuring. More specific substrings (`"engine/retriever.rs"`) mitigate false positives.
- **Recall excludes coverage checks:** `expected_chunk_types`, `expected_projects`, `min_relevant_results`, and `excluded_files` are boolean checks reported alongside recall, not part of the recall numerator. This keeps the recall metric focused on "did we find the right content?"
- **`mod harness` in `main.rs` (not `lib.rs`):** V3.2 will extract to `lib.rs` for the second binary target. No premature structural refactoring.
- **`#[allow(dead_code)]` on harness module:** Types are only consumed by tests now; V3.2 binary will remove the need for this.

### Test Results

152 tests pass (15 new + 137 existing), 0 failures, 5 ignored (require external resources). Workspace-wide clippy clean with `-D warnings`.

### Dataset Coverage

| Category | Count | Primary assertions |
|----------|-------|--------------------|
| Hero | 5 | All dimensions — regression anchors (3 from V1, 2 from V2) |
| Overview | 7 | `expected_chunk_types`, `expected_projects` |
| Implementation | 11 | `expected_files`, `expected_identifiers` |
| Relationship | 5 | `expected_files` (callers/callees) |
| Comparison | 4 | `expected_files` (both subjects) |
| Smoke | 7 | Only `min_relevant_results` and/or `excluded_files` |
| Edge cases | 4 | Empty expectations, ambiguous, multi-project, very specific |

---

## 2026-03-26: GitHub Pages Demo

### Summary

Deployed code-rag-chat as a fully static GitHub Pages demo. The Leptos WASM frontend's `standalone` feature flag switches from calling a backend API to running the entire RAG pipeline in-browser — embedding queries via transformers.js, brute-force vector search, intent classification, and context building all run client-side. The shared `code-rag-engine` crate ensures both Docker and GitHub Pages deployments compile the same algorithms. LLM generation (Gemini) is optional, unlocked via Google OAuth2 or API key.

### Motivation

- **Portfolio demo without Docker**: Visitors can try the RAG pipeline directly in their browser — no clone, no build, no backend.
- **Automatic sync**: Improvements to intent classification, context building, or retrieval routing in `code-rag-engine` automatically apply to both Docker and GitHub Pages deployments.
- **LLM generation is optional**: The retrieval pipeline (embedding, intent classification, vector search, context formatting) works without any API key. Auth unlocks Gemini-powered answers.

### Architecture

```
code-rag-engine (shared, pure Rust, no I/O)
├── intent.rs     — classify(), route(), cosine_similarity()
├── context.rs    — build_context(), build_prompt(), SYSTEM_PROMPT
├── config.rs     — RetrievalConfig, EngineConfig, RoutingTable
└── retriever.rs  — ScoredChunk<T>, RetrievalResult, distance_to_relevance()

code-rag-ui (Leptos WASM)
├── [default]     — api.rs calls /chat endpoint (Docker)
└── [standalone]  — runs engine in-browser:
    ├── embedder.rs    — wasm-bindgen bridge to transformers.js
    ├── data.rs        — load pre-computed ChunkIndex from static JSON
    ├── search.rs      — brute-force L2 vector search
    ├── gemini.rs      — direct Gemini REST API (optional, needs auth)
    ├── auth.rs        — OAuth2 PKCE + API key, localStorage persistence
    ├── standalone_api.rs — full + rag-only pipeline variants
    └── components/auth_panel.rs — sign-in UI (Google OAuth2 + API key)

static/embedder.js — transformers.js wrapper (BGE-small-en-v1.5 via CDN)
```

### What Changed

**New crate: `code-rag-engine`** (`crates/code-rag-engine/`)
- Extracted pure, platform-agnostic functions from `src/engine/` — no I/O, no HTTP, compiles to wasm32.
- `IntentClassifier::build()` takes a closure `impl FnMut(&[&str]) -> Result<Vec<Vec<f32>>, E>` instead of concrete `Embedder`. Caller provides their own embedding function.
- Added `IntentClassifier::from_prototypes()` for loading pre-computed embeddings.
- Added `retriever::to_retrieval_result()` helper for building results from raw search tuples.
- 25 tests (includes 3 new closure-based classifier tests).

**Updated: `src/engine/`**
- Now re-exports from `code-rag-engine`. Keeps only I/O-bound `retrieve()` and platform-specific `EngineError`.
- `src/api/state.rs` passes closure to `IntentClassifier::build`.
- `src/api/dto.rs` imports directly from `code_rag_engine`.

**New feature: `code-rag-ui --features standalone`**
- `data.rs` — `ChunkIndex` type, `load_index()` fetches pre-computed JSON.
- `search.rs` — brute-force L2 search over `EmbeddedChunk<T>` vectors.
- `gemini.rs` — direct Gemini REST API client, supports both `AuthMethod::ApiKey` and `AuthMethod::OAuth2`.
- `auth.rs` — PKCE flow helpers (code verifier, SHA-256 challenge, token exchange), localStorage persistence.
- `standalone_api.rs` — two variants: `send_chat_standalone()` (full pipeline with Gemini) and `send_chat_rag_only()` (retrieval without LLM, works unauthenticated).
- `embedder.rs` — wasm-bindgen bridge calling `window.__codeRagEmbedQuery()` from transformers.js.
- `components/auth_panel.rs` — Google OAuth2 sign-in button + API key input, handles PKCE callback on page load.
- `main.rs` — feature-gated: standalone mode loads `ChunkIndex` from `/index.json`, pre-warms embedder, provides context signals; default mode fetches from backend API.
- `chat_view.rs` — feature-gated submit handler: standalone embeds query in-browser → runs pipeline; default calls HTTP `/chat`.
- Default build (no flag) unchanged — still calls `/chat` API.

**New subcommand: `code-raptor export`**
- Reads all 4 chunk types from LanceDB including embedding vectors.
- Embeds intent prototype queries and includes them in the export.
- Outputs single JSON file matching the `ChunkIndex` format.
- Usage: `code-raptor export --db-path data/portfolio.lance --output crates/code-rag-ui/static/index.json`

**New: `static/embedder.js`**
- Thin wrapper around transformers.js v3.8.1 (loaded via CDN, no npm/bundler).
- Model: `Xenova/bge-small-en-v1.5` — same 384-dim vectors as native fastembed, fully compatible.
- Lazy-loads on first query; model cached in IndexedDB (~33MB).
- Exposes `window.__codeRagEmbedQuery()` and `window.__codeRagInitEmbedder()`.

**New: `config/targets.json`**
- Configurable list of repos for CI ingestion (repo URL + project name).
- Workflow loops over entries, cloning and ingesting each into the same LanceDB.

**Rewritten: `.github/workflows/gh-pages.yml`**
- Installs `protobuf-compiler` (required by lance-encoding).
- Reads `config/targets.json`, clones each repo, runs ingestion → export → `trunk build --features standalone` → deploy.
- Injects `GOOGLE_OAUTH_CLIENT_ID` from GitHub secrets at build time.

**Updated: `dockerfile/Dockerfile`**
- Added `COPY` for `static/` directory (embedder.js).

### Key Design Decisions

1. **Shared crate, no traits** — `code-rag-engine` contains only pure functions and data types. No trait abstractions, no generics over I/O. Both platforms call the same functions with different data sources.

2. **Feature flag, not separate crate** — `code-rag-ui` with `standalone` feature reuses all UI components. Only the data layer switches (API calls vs in-browser pipeline).

3. **Optional LLM generation** — retrieval results (intent, chunks, sources, scores) display without auth. Both Docker and GitHub Pages modes benefit. Auth unlocks Gemini answers.

4. **Closure-based `IntentClassifier::build()`** — avoids trait overhead while decoupling from concrete `Embedder`. The WASM build uses pre-computed prototypes, native passes fastembed, export tool pre-computes them.

5. **transformers.js over ort-WASM** — ort's WASM target is experimental. transformers.js v3.8.1 is battle-tested, runs the same BGE-small-en-v1.5 model, loads from CDN with no build tooling, and caches in IndexedDB. Thin JS interop via `#[wasm_bindgen]`.

6. **Config-driven ingestion targets** — `config/targets.json` lists repos to ingest in CI, making it easy to add projects without editing the workflow.

### Remaining Work

- End-to-end testing of GitHub Pages deployment
- OAuth2 redirect URI configuration in GCP Console (`https://paulxiep.github.io/code-rag/`)
- Progress indicator for first-time model download (~33MB)

### Test Results

135 tests pass across all workspace crates (up from 132 — 3 new closure-based classifier tests in `code-rag-engine`).

---

## 2026-03-25: Leptos Migration — WASM Frontend

### Summary

Replaced the server-rendered htmx/Askama frontend with a Leptos WASM SPA (client-side rendered). The frontend compiles to WebAssembly and runs entirely in the browser, communicating with the Axum backend via JSON API. This is Step 1 toward a fully static GitHub Pages demo where the entire RAG pipeline runs in-browser.

### Motivation

- **GitHub Pages demo**: The end goal is a static demo that runs the full RAG pipeline (embedding, vector search, intent classification) in WASM without a backend. Leptos WASM is the foundation.
- **Performance and file size**: Leptos produces ~100-300KB gzipped WASM bundles (no virtual DOM). Much smaller than a React/JS equivalent.
- **Architectural coherence**: One language (Rust) for the entire stack — engine, API, and UI.
- **Theme consistency**: Visual design matches the paulxie Astro portfolio (Atkinson font, `#2337ff` accent, same spacing and component patterns).

### Architecture

```
Browser (WASM)                    Axum Server
┌──────────────────┐              ┌──────────────────┐
│  Leptos SPA      │  fetch()     │  JSON API        │
│  ├─ ChatView     │────────────→ │  POST /chat      │
│  ├─ SourcesPanel │              │  GET /projects   │
│  ├─ IntentBadge  │              │  GET /health     │
│  └─ ProjectTags  │              │                  │
└──────────────────┘              └──────────────────┘
```

Axum serves the WASM bundle via `ServeDir` with SPA fallback. The `UI_DIST` env var points to the trunk build output.

### What Changed

**New crate**: `crates/code-rag-ui/`
- Leptos 0.8 CSR app with trunk build tooling
- Components: `ChatView`, `SourcesPanel`, `IntentBadge`
- API client: `gloo-net` fetch to Axum JSON endpoints
- CSS: Portfolio design tokens (Atkinson font, accent colors, card/tag patterns)

**Removed**:
- `src/api/web.rs` — Askama HTML form handler
- `templates/` — Askama HTML templates
- `static/` — htmx.min.js, old CSS
- `askama` dependency

**Modified**:
- `src/api/mod.rs` — Removed HTML routes, added `ServeDir` + SPA fallback
- `Cargo.toml` — Removed `askama`, added `code-rag-ui` to workspace

### Design Decisions

| Decision | Rationale |
|----------|-----------|
| Leptos over Yew/Dioxus | Smallest WASM bundles (fine-grained reactivity, no virtual DOM) |
| CSR-only (no SSR) | Targeting GitHub Pages — must work as static files |
| `gloo-net` over `reqwest` | Lighter WASM footprint for HTTP requests |
| Portfolio theme reuse | Consistent visual identity across paulxie projects |
| `ServeDir` + SPA fallback | Single binary serves both API and frontend |

### Separation of Concerns (Step 2 prep)

The UI uses a simple API client module (`api.rs`). In Step 2 (GitHub Pages demo), this will be replaced by a `RagEngine` trait with two implementations:
- `ApiEngine`: Current HTTP client (Step 1)
- `WasmEngine`: In-browser embedding (tract-onnx), vector search, intent classification (Step 2)

The UI layer only depends on the trait, not on how the engine is implemented.

### Verification

- `trunk build` compiles WASM bundle successfully
- `cargo check` — server compiles without Askama
- `cargo test` — all 28 tests pass, 0 regressions
- UI components: ChatView, SourcesPanel, IntentBadge all render

### Test Results

```
test result: ok. 28 passed; 0 failed; 8 ignored; 0 measured; 0 filtered out
```

## 2026-02-08: V2.3 Retrieval Traces (V2 Phase 3)

### Summary

Made retrieval quality visible by surfacing all 4 chunk types (code, readme, crate, module_doc) with relevance scores in API responses and the htmx UI. Portfolio differentiator: the system shows its work instead of acting as a black box. Redesigned `SourceInfo` from code-only to universal, extracted shared source-building logic into `dto.rs` to eliminate handler duplication, and switched search API to scored-only (Option B) since the distance column is already computed by LanceDB on every vector search.

### Architecture

```
LanceDB vector_search()
    |
    v
RecordBatch with _distance column (Float32Array, L2 distance)
    |
    v
vector_store.rs: extract_*_from_batch() -> Vec<(ChunkType, f32)>
    |
    v
retriever.rs: distance -> relevance (1.0 / (1.0 + dist)) -> ScoredChunk<T>
    |
    v
RetrievalResult { Vec<ScoredChunk<T>>..., intent: QueryIntent }
    |
    +-- context.rs: build LLM context (accesses scored.chunk.field, ignores score)
    |
    +-- dto.rs: build_sources() -> Vec<SourceInfo>, sorted by relevance
        |
        v
    ChatResponse { answer, sources, intent }
```

Two-consumer split: context builder uses chunk content only (SoC — LLM doesn't need relevance metadata). Source builder maps all chunk types to uniform `SourceInfo` with scores for API/UI display.

### Changes

| File | Change |
|------|--------|
| `crates/coderag-store/src/vector_store.rs` | All 4 `extract_*_from_batch()` return `Vec<(T, f32)>` with `_distance` column (fallback 0.0). All `batches_to_*()` and `search_*()` methods return scored tuples. `search_all()` returns 4-tuple of `Vec<(T, f32)>`. Scored-only API (Option B). |
| `src/engine/retriever.rs` | Added `ScoredChunk<T> { chunk, score }`, `distance_to_relevance()`, `to_scored()`. `RetrievalResult` now contains `Vec<ScoredChunk<T>>` + `intent: QueryIntent`. `retrieve()` takes `intent` param (passed through). 3 unit tests for distance→relevance conversion. |
| `src/engine/context.rs` | All `format_*_section()` accept `&[ScoredChunk<T>]`. Mechanical `chunk.field` → `scored.chunk.field`. All 8 test fixtures updated with `scored()` helper. |
| `src/api/dto.rs` | Redesigned `SourceInfo` (chunk_type, path, label, project, relevance, relevance_pct, line). `ChatResponse.intent: QueryIntent` (direct serde, not String). Extracted `build_sources()` + 4 `SourceInfo::from_scored_*()` constructors. 7 unit tests. |
| `src/api/handlers.rs` | Simplified to: `retrieve(..., intent)` → `dto::build_sources(&result)` → `ChatResponse { answer, sources, intent }`. No inline source-mapping loops. |
| `src/api/web.rs` | Same pattern using shared `build_sources()`. Added `intent: String` to `MessageFragment` (Askama boundary conversion). |
| `templates/partials/message.html` | Chunk type badges with CSS classes, relevance %, intent in summary, conditional line numbers. |

### Key Design Decisions

1. **Scored-only search API (Option B)**: Modified existing `search_code()`, `search_all()` in-place to return `Vec<(T, f32)>` instead of adding `_scored()` variants alongside. Rationale: only `retriever.rs` calls these methods (code-raptor never searches), zero performance cost (LanceDB computes `_distance` on every vector search anyway), single code path.
2. **`build_sources()` in dto.rs**: Source-mapping logic extracted from handlers into `dto.rs` with `SourceInfo::from_scored_*()` constructors. Eliminates duplication between `handlers.rs` and `web.rs`. Handler becomes pure orchestration.
3. **`ChatResponse.intent: QueryIntent`**: Direct serde serialization instead of converting to String. `QueryIntent` already derives `Serialize` with `#[serde(rename_all = "snake_case")]`.
4. **`relevance_pct: u8` pre-computed**: Askama templates can't do inline arithmetic (`{{ val * 100.0 }}`). Pre-computed as `(score * 100.0).round() as u8` in `from_scored_*()` constructors.
5. **Context builder ignores scores**: `format_*_section()` functions access `scored.chunk.field` but never use `scored.score`. Correct SoC — context is about content for the LLM, not ranking metadata for the user.
6. **Distance → relevance formula**: `1.0 / (1.0 + dist)` — simple, monotonically decreasing, metric-agnostic. Maps [0, ∞) → (0, 1]. No assumptions about distance metric.

### Refinements vs Original Spec

| Issue | Original Spec | Implementation |
|-------|--------------|----------------|
| Source-building duplication | 4 `for` loops copy-pasted in handlers.rs + web.rs | Single `build_sources()` in dto.rs |
| Intent serialization | `serde_json::to_value()` dance → String | Direct `QueryIntent` on `ChatResponse` |
| Search API duplication | 8 new `_scored()` functions alongside 8 existing | Scored-only (Option B) — modified in-place |
| `SourceInfo` mapping | Procedural in handler | `from_scored_*()` constructors on `impl SourceInfo` |

### API Breaking Changes

`ChatResponse` gains `intent` field. `SourceInfo` redesigned: `file` → `path`, `function` → `label`, new fields `chunk_type`, `relevance`, `relevance_pct`. Acceptable: pre-v1.0, single consumer (htmx frontend, updated simultaneously).

### Test Results

All 132 tests pass (12 ignored):
- code-raptor: 78 unit + 9 integration tests (unchanged)
- coderag-store: 8 unit tests (1 updated for tuple destructuring)
- coderag-types: 9 tests (unchanged)
- portfolio-rag-chat: 28 unit tests (3 new retriever + 7 new dto + 8 updated context)
- `cargo fmt --all` clean
- `cargo clippy --workspace` clean (0 warnings)

### What This Enables

- Users see all retrieved sources (not just code) with chunk type badges and relevance percentages
- Cross-type ranking: a highly relevant README can rank above a less relevant code chunk
- Intent visible in response: users understand how their query was classified
- Foundation for V3 quality harness: relevance scores enable recall@K measurement

**Crates:** coderag-store, portfolio-rag-chat

---

## 2026-02-08: V2.2 Intent Classification + Query Routing (V2 Phase 2)

### Summary

Embedding-based intent classification with query routing. Replaced initial keyword-based classifier with cosine-similarity classification against pre-computed prototype query embeddings. Restructured the handler pipeline to embed once and reuse the vector for both classification and retrieval, reducing Mutex hold time from ~50ms to ~5ms.

**Iteration history:** Initially implemented with keyword heuristics (substring matching). Discovered regression — Overview's `code_limit: 2` starved code chunks, causing wrong answers. Fixed code_limit to 5 across all intents, then upgraded classification mechanism from keywords to embeddings.

### Architecture

```
User Query
    │
    ▼
lock embedder
    embed_one(query)                         ← ~5ms, produces Vec<f32> (384-dim)
unlock embedder
    │
    ▼
intent::classify(query_vec, &IntentClassifier)  ← cosine similarity vs prototype embeddings
    │
    ▼
ClassificationResult { intent, confidence: f32 }
    │
    ▼
intent::route(intent, &RoutingTable)          ← HashMap lookup, fallback to default
    │
    ▼
RetrievalConfig { code_limit, readme_limit, crate_limit, module_doc_limit }
    │
    ▼
retriever::retrieve(query_vec, store, &config)   ← pure vector search, no re-embedding
```

Three wins from the restructure:
1. **Semantic classification** — cosine similarity against prototype embeddings, not substring matching
2. **Mutex held ~5ms** — down from ~50ms+ (embedding was inside retriever)
3. **Retriever is pure search** — takes `&[f32]`, no `&mut Embedder` dependency

### Changes

| File | Change |
|------|--------|
| `src/engine/intent.rs` | Removed `IntentRule`, `IntentConfig`, keyword `classify()`. Added `IntentClassifier` (prototype embeddings), `cosine_similarity()`, prototype constants, embedding-based `classify()`. 17 tests (4 cosine + 5 routing + 1 serialization + 7 embedding). |
| `src/engine/config.rs` | Removed `intent: IntentConfig` field. `EngineConfig` now contains only `routing: RoutingTable`. `#[derive(Default)]`. |
| `src/engine/retriever.rs` | Signature: `(&[f32], &VectorStore, &RetrievalConfig)` instead of `(&str, &mut Embedder, &VectorStore, &RetrievalConfig)`. Removed internal embed step. |
| `src/api/state.rs` | Added `classifier: IntentClassifier` to `AppState`. Built at startup before Mutex wraps embedder. |
| `src/api/handlers.rs` | Embed once → classify → route → retrieve pipeline. Mutex held only for `embed_one()`. |
| `src/api/web.rs` | Same pipeline restructure with `match`-based error handling for embed_one. |

### Classification Mechanism

**Prototype queries** — ~5-6 static `&str` per intent, embedded at startup (~200ms one-time cost):

| Intent | Prototype examples |
|--------|-------------------|
| Overview | "What is this project?", "Give me an overview", "What is the architecture?" |
| Implementation | "How does this function work?", "Show me the implementation" |
| Relationship | "What calls this function?", "What depends on this?" |
| Comparison | "Compare A and B", "What are the differences between X and Y?" |

**Algorithm:** For each intent, compute max cosine similarity between query embedding and that intent's prototype embeddings. Highest max wins. Threshold 0.3 — below this, falls back to Implementation default.

**Advantage over keywords:** "Explain how the retriever implements caching" — keywords would match "explain" → Overview. Embedding similarity correctly classifies as Implementation.

### Routing Table

| Intent | code | readme | crate | module_doc | Total |
|--------|------|--------|-------|------------|-------|
| Overview | 5 | 3 | 3 | 3 | 14 |
| Implementation | 5 | 1 | 1 | 2 | 9 |
| Relationship | 5 | 1 | 2 | 2 | 10 |
| Comparison | 5 | 2 | 3 | 2 | 12 |
| Default | 5 | 2 | 3 | 3 | 13 |

`code_limit` fixed at 5 across all intents. Differentiation in supplementary context only. Revisit once V3 quality harness measures recall@5 per intent.

### Key Design Decisions

1. **Embed once, reuse everywhere**: Query embedding computed once in handler, passed to both `classify()` and `retrieve()`. Eliminates redundant embedding inside retriever.
2. **`IntentClassifier` as runtime object**: Holds `Vec<Vec<f32>>` prototypes. Requires `&mut Embedder` to construct → lives in `AppState`, not `EngineConfig`.
3. **Retriever becomes pure search**: `retrieve()` takes `&[f32]`, no longer owns embedding responsibility. SoC improved.
4. **Prototype queries as static data**: Same declarative pattern as keywords — `&[&str]` constants, not if-else chains.
5. **`confidence: f32`** replaces `match_count: usize`: Cosine similarity score enables future threshold tuning and analytics.

### Test Results

All 19 unconditional tests pass (8 ignored):
- engine::intent: 4 cosine similarity + 5 routing + 1 serialization (unconditional)
- engine::intent: 7 embedding classification tests (`#[ignore]`, require model download) — all pass
- engine::context: 9 tests (existing, unchanged)
- engine::generator: 1 test (ignored, requires GEMINI_API_KEY)

Key regression test: `test_classify_paraphrase_implementation` — "Explain how the retriever implements caching" → Implementation (not Overview). This would FAIL with keyword matching.

### What This Enables

- Semantic understanding of query intent, not brittle substring matching
- Paraphrased queries classify correctly (the key weakness of keywords)
- Confidence scores for future analytics and multi-intent exploration
- Foundation for V3 quality harness correlation: do high-confidence classifications produce better recall?

**Crate:** portfolio-rag-chat

---

## 2026-02-07: V2.1 Inline Call Context (V2 Phase 1)

### Summary
Enriched embedding text with `Calls: foo, bar` lines so functions become semantically closer to relationship queries in vector space. Implemented `extract_calls()` on the `LanguageHandler` trait for all three languages (Rust, Python, TypeScript), extended the parser fold to return `(CodeChunk, Vec<String>)` tuples, and threaded an ephemeral `HashMap<String, Vec<String>>` side-channel from `run_ingestion` through `embed_and_store_code`. Calls bypass reconcile and are discarded after embedding — they never touch `coderag-types` or the database schema.

### Architecture: Ephemeral Side-Channel

```
run_ingestion()
  ├─ IngestionResult     → reconcile → embed_and_store_all
  └─ HashMap<chunk_id,   ─────────────────────┐
       Vec<calls>>                             │
                                               ▼
                              embed_and_store_code() enriches embedding text:
                                "foo (rust)\nfn foo() { bar(); }\nCalls: bar"
                              then HashMap is discarded
```

Calls are ephemeral enrichment data — they don't belong on `CodeChunk` (the cross-crate contract in `coderag-types`). Track C will have its own persistent `call_edges` table for structured graph queries.

### Continuity with V1.5

Follows the same four-step extension pattern:
1. Trait method (`extract_calls` on `LanguageHandler`)
2. Per-handler implementation (Rust, Python, TypeScript)
3. Fold extension (`parser.rs` 5-tuple → 6-tuple)
4. Downstream consumption

Diverges at step 4: V1.5 stored docstrings on `CodeChunk` (persistent), V2.1 uses an ephemeral HashMap (transient enrichment only).

### Changes

| File | Change |
|------|--------|
| `crates/code-raptor/src/ingestion/language.rs` | Added `extract_calls()` default trait method returning `Vec::new()` |
| `crates/code-raptor/src/ingestion/languages/rust.rs` | Implemented `extract_calls` + `collect_calls_recursive` helper, 5 unit tests |
| `crates/code-raptor/src/ingestion/languages/python.rs` | Implemented `extract_calls` + `collect_calls_recursive` helper, 4 unit tests |
| `crates/code-raptor/src/ingestion/languages/typescript.rs` | Implemented `extract_calls` + `collect_calls_recursive` helper, 4 unit tests |
| `crates/code-raptor/src/ingestion/parser.rs` | Extended fold 5-tuple → 6-tuple with `Vec<String>`, `analyze_with_handler` returns `Vec<(CodeChunk, Vec<String>)>`, added `type RawMatch` alias, added `test_calls_pipeline`, updated ~11 existing tests with `chunks_only()` helper |
| `crates/code-raptor/src/ingestion/mod.rs` | `process_code_file` returns `(Vec<CodeChunk>, HashMap)`, `run_ingestion` returns `(IngestionResult, HashMap)`, added `type CallsMap` alias, updated 4 tests |
| `crates/code-raptor/src/main.rs` | Threaded `calls_map` through `run_full_ingestion`, `run_incremental_ingestion`, `embed_and_store_all`, `embed_and_store_code`; lookup by `chunk_id` in embedding loop |
| `crates/coderag-store/src/embedder.rs` | Added `calls: &[String]` parameter to `format_code_for_embedding`, appends `Calls:` line if non-empty, 2 new tests + 2 updated tests |
| `crates/code-raptor/tests/incremental_ingestion.rs` | Updated all 9 integration tests to destructure `(result, _)` from `run_ingestion` |

### Per-Language Call Extraction

| Language | AST Node | Direct Call | Method Call |
|----------|----------|-------------|-------------|
| Rust | `call_expression` | `function: identifier` → `foo()` | `function: field_expression > field: field_identifier` → `self.bar()` |
| Python | `call` | `function: identifier` → `foo()` | `function: attribute > attribute: identifier` → `self.bar()` |
| TypeScript | `call_expression` | `function: identifier` → `foo()` | `function: member_expression > property: property_identifier` → `obj.bar()` |

Each handler walks the body node descendants via `TreeCursor` recursion, sorts + dedups results.

### Scope Exclusions

- No macro invocations (Rust `macro_rules!` calls)
- No variable-bound calls (`let f = bar; f()`)
- No cross-file resolution (Track C scope)
- No generic/template specialization calls

### Breaking Change: `analyze_with_handler` Return Type

`Vec<CodeChunk>` → `Vec<(CodeChunk, Vec<String>)>`

This broke ~30 tests across the codebase. All were mechanical fixes: add a `chunks_only()` helper per test module that strips the calls via `.map(|(c, _)| c).collect()`, or destructure `let (result, _) = run_ingestion(...)`.

### Key Design Decisions

1. **Ephemeral HashMap, not on CodeChunk**: SoC — `coderag-types` is the cross-crate data contract. Embedding enrichment data doesn't belong on the shared type. Track C will have its own persistent storage.
2. **`type CallsMap` and `type RawMatch` aliases**: Introduced to satisfy `clippy::type_complexity` without structural changes.
3. **Declarative `unzip` in `run_ingestion`**: Preferred over imperative `fold` at file-count scale. `embed_and_store_code` keeps its imperative batching (EMBEDDING_BATCH_SIZE = 25) where memory matters.
4. **Calls appended to embedding text, not prepended**: Embedding models weight earlier text more heavily — identifier, docstring, and code content should dominate the vector, with calls as supplementary signal.

### Gotchas Found During Implementation

1. **Missing closing brace in typescript.rs**: `collect_calls_recursive` was missing its `}` before `#[cfg(test)]` — caught by compilation.
2. **`flat_map(|m| m)` → `flatten()`**: Clippy flagged `flat_map_identity` in `run_ingestion`'s call map merge.
3. **6 `collapsible_if` warnings**: All three handler `collect_calls_recursive` functions had nested `if node.kind() == X { if let Some(func) = ... }` — collapsed with `&&` let chains.
4. **Integration tests not updated**: `tests/incremental_ingestion.rs` called `run_ingestion` without destructuring the new tuple return — 26 compilation errors, all fixed by `let (result, _) = ...`.

### Test Results

All 95 tests pass (0 warnings):
- code-raptor: 78 unit tests (13 new call extraction + 1 pipeline + mechanical updates)
- code-raptor: 9 integration tests (updated for tuple return)
- coderag-store: 8 tests (2 new call format + 2 updated signature)
- `cargo fmt --all` clean
- `cargo clippy --workspace` clean (0 warnings)

### Deployment

Requires `code-raptor ingest <repo> --full` after deployment. Content hashes are file-level — call context changes the embedding text but not the hash, so incremental mode won't detect the change.

### What This Enables

Queries like "what functions call process_data?" or "show me callers of authenticate" will produce better vector matches because the embedding text now contains `Calls: process_data` or `Calls: authenticate`. This is a probabilistic improvement — not a precise graph query (that's Track C + V2.3 query routing).

**Crates:** code-raptor, coderag-store

---

## 2026-02-07: V1.5 Docstring Extraction (V1 Milestone Complete)

### Summary
Wired `extract_docstring()` into the parser pipeline and implemented it for all three language handlers (Rust, Python, TypeScript). The `docstring` field is now populated with real documentation instead of `None`. This completes the V1 (Indexing Foundation) milestone.

### Three Concerns (SoC)

1. **Parser wiring** — restructured `analyze_with_handler()` fold closure to call `handler.extract_docstring()` while the tree-sitter Node is still alive
2. **Handler implementations** — implemented for RustHandler (`///`, `#[doc]`) and PythonHandler (triple-quoted string in body via AST traversal)
3. **TypeScript verification** — V1.4's JSDoc extraction was dead code; V1.5 activated it via parser wiring and verified with pipeline tests

### Changes

| File | Change |
|------|--------|
| `crates/code-raptor/src/ingestion/parser.rs` | Extended fold tuple to `(String, String, String, usize, Option<String>)`, call `handler.extract_docstring()` inside fold, added 4 cross-language pipeline tests |
| `crates/code-raptor/src/ingestion/languages/rust.rs` | Implemented `extract_docstring` for `///` and `#[doc = "..."]`, added 7 unit tests |
| `crates/code-raptor/src/ingestion/languages/python.rs` | Implemented `extract_docstring` with AST traversal + `dedent_docstring()`, added 6 unit tests |
| `crates/code-raptor/src/ingestion/languages/typescript.rs` | Added 5 pipeline tests verifying JSDoc through `analyze_with_handler` |
| `crates/code-raptor/src/ingestion/language.rs` | Updated stale doc comments (V1.4 references to V1.5) |
| `src/engine/context.rs` (portfolio-rag-chat) | Added docstring display to `format_code_section()`, added context test |

### Extraction Strategies by Language

| Language | Strategy | Patterns |
|----------|----------|----------|
| Rust | Scan backwards from `node.start_position().row` | `///` outer doc, `#[doc = "..."]` attribute form. Skips `#[derive]`/`#[cfg]` |
| Python | AST traversal into function/class body | `"""..."""` and `'''...'''` triple-quoted strings. First `expression_statement` → `string` node. Dedented via `dedent_docstring()` |
| TypeScript | Scan backwards for JSDoc blocks (V1.4) | `/** ... */` multi/single-line. Filters out `@param`, `@returns` |

### Key Design Decisions

1. **Docstring extracted inside `fold` closure** — Node lifetime constraint: tree-sitter Nodes are only valid during fold iteration. Must extract before the tuple is created.
2. **`//!` (inner doc) excluded from RustHandler** — Already handled by `extract_module_docs()` in parser.rs. SoC: per-item docs vs module-level docs.
3. **Python uses AST, not line scanning** — Unlike Rust/TypeScript which scan backwards from the node, Python docstrings live inside the body. Tree-sitter AST traversal (`node → body → first expression_statement → string`) is the correct approach.
4. **Downstream already ready** — `format_code_for_embedding()`, Arrow schema, VectorStore roundtrip, and retriever all handled `Option<String>` docstrings since V1.1. Only context display needed a small addition.

### Gotchas Found During Implementation

1. **Node lifetime in `fold` closure** — Only primitives survived into the tuple. Must call `extract_docstring()` inside fold where Node is alive.
2. **Clippy: `if_same_then_else`** — Python's `parse_python_docstring()` had identical blocks for `"""` vs `'''` and `"` vs `'`. Consolidated with `||` conditions.
3. **Clippy: `collapsible_if`** — Rust's `#[doc]` parsing had nested `if let` chains. Collapsed with `let`-chaining.
4. **TypeScript arrow function `@body` offset** — `@body` captures `arrow_function` node, not `lexical_declaration`. Works for single-line declarations; accepted limitation for rare multi-line splits.

### Test Results

All 97 tests pass (0 warnings):
- code-raptor: 64 unit tests (7 new Rust + 6 new Python + 5 new TypeScript pipeline + 4 new cross-language pipeline)
- code-raptor: 9 integration tests
- coderag-store: 6 tests
- coderag-types: 9 tests
- portfolio-rag-chat: 9 tests (1 new docstring context test)
- `cargo fmt --all` clean
- `cargo clippy --workspace` clean (0 warnings)

### V1 Milestone Status

V1 (Indexing Foundation) is now complete:
- V1.1: Schema Foundation (chunk_id, content_hash, embedding_model_version)
- V1.2: LanguageHandler Trait (trait-based language abstraction)
- V1.3: Incremental Ingestion (file-level change detection, reconcile)
- V1.4: TypeScript Support (TypeScriptHandler + JSDoc extraction)
- V1.5: Docstring Extraction (wired pipeline + Rust/Python/TypeScript handlers)

**Crate:** code-raptor, portfolio-rag-chat

---

## 2026-02-07: V1.4 TypeScript Support

### Summary
Added TypeScript as a supported language using the V1.2 LanguageHandler trait. TypeScriptHandler uses the TSX grammar (superset of TS/JS/JSX/TSX) and captures 8 node types: functions, arrow functions (const/let/var), classes, methods, interfaces, type aliases, and enums. JSDoc extraction is implemented on the handler but remains unwired in parser.rs until V1.5 (SoC: handler declares capability, parser wires on its own schedule).

### Changes

| File | Change |
|------|--------|
| `crates/code-raptor/Cargo.toml` | Added `tree-sitter-typescript = "0.23"` |
| `crates/code-raptor/src/ingestion/languages/typescript.rs` | **NEW** — TypeScriptHandler + 15 unit tests |
| `crates/code-raptor/src/ingestion/languages/mod.rs` | Registered TypeScriptHandler in handler vec |
| `crates/code-raptor/src/ingestion/parser.rs` | Fixed `.js` test assertion (`is_none()` → `is_some()`), added `.go` for `is_none()` |
| `crates/code-raptor/src/ingestion/mod.rs` | Added `test_run_ingestion_typescript` integration test |
| `portfolio-rag-chat/development_plan.md` | Fixed V1.4/V1.5 ordering (was swapped) |

### TypeScript Query Patterns

| Pattern | Captures | Example |
|---------|----------|---------|
| `function_declaration` | Named functions | `function foo() {}` |
| `arrow_function` in `lexical_declaration` | Arrow functions (const/let) | `const foo = () => {}` |
| `arrow_function` in `variable_declaration` | Arrow functions (var) | `var foo = () => {}` |
| `class_declaration` | Classes | `class Foo {}` |
| `method_definition` | Class methods | `class { foo() {} }` |
| `interface_declaration` | Interfaces | `interface Foo {}` |
| `type_alias_declaration` | Type aliases | `type Foo = ...` |
| `enum_declaration` | Enums | `enum Foo { A, B }` |

Exported items (`export function foo()`, `export class Foo`) are captured by the base patterns — no separate export patterns needed.

### Key Design Decisions

1. **TSX grammar for all JS/TS**: `LANGUAGE_TSX` is a superset that handles `.ts`, `.tsx`, `.js`, `.jsx` — avoids maintaining separate grammars
2. **`language` field always "typescript"**: Accepted for V1.4. Not worth per-file language detection complexity
3. **`extract_docstring` implemented but dead**: SoC — handler declares JSDoc extraction capability, parser.rs hardcodes `docstring: None` until V1.5 wires it
4. **No redundant export patterns**: Tree-sitter queries match nested nodes, so `function_declaration` already matches inside `export_statement`. Dedup via `(identifier, start_line)` handles any duplicates

### Gotchas Found During Implementation

1. **`extract_docstring` is dead code until V1.5** — parser.rs line 96 hardcodes `docstring: None`. JSDoc tests must call `handler.extract_docstring()` directly, not expect docstrings in `CodeChunk` output from the pipeline
2. **`tree-sitter-typescript` version** — v0.23.2 uses `tree-sitter-language = "0.1"` as bridge crate, compatible with `tree-sitter = "0.26"` (same pattern as rust 0.24 and python 0.25)
3. **Existing test broke** — `parser.rs` had `assert!(handler_for_path(Path::new("test.js")).is_none())`, fixed to `is_some()` and added `test.go` for `is_none()`
4. **Missing `enum_declaration`** — original plan omitted TypeScript enums, added to query patterns
5. **Export patterns were redundant** — removed export-wrapped patterns, verified with `test_parse_exported_function`
6. **Clippy: identical `if` branches** — consolidated `line.starts_with("//")` branch into general break condition in `extract_docstring`

### Test Results

All 51 tests pass (0 warnings):
- code-raptor: 42 unit tests (15 new TypeScript + 27 existing)
- code-raptor: 9 integration tests (1 new TypeScript)
- `cargo fmt` clean
- `cargo clippy` clean

### Unit Tests (15 in `typescript.rs`)

| Test | Validates |
|------|-----------|
| `test_extensions` | All 4 extensions: `.ts`, `.tsx`, `.js`, `.jsx` |
| `test_parse_function_declaration` | `function foo()` → identifier "foo", node_type "function_declaration" |
| `test_parse_arrow_function` | `const add = () => ...` → identifier "add" |
| `test_parse_arrow_function_var` | `var legacy = () => {}` → identifier "legacy" |
| `test_parse_class_with_methods` | Class + methods captured separately |
| `test_parse_interface` | `interface User {}` → node_type "interface_declaration" |
| `test_parse_type_alias` | `type Result<T> = ...` → node_type "type_alias_declaration" |
| `test_parse_enum` | `enum Direction {}` → node_type "enum_declaration" |
| `test_parse_exported_function` | `export function` captured by base pattern |
| `test_parse_react_component` | TSX function component captured |
| `test_parse_arrow_react_component` | TSX arrow component captured |
| `test_jsdoc_single_line` | `/** text */` → extracts description (calls handler directly) |
| `test_jsdoc_multiline` | Multi-line JSDoc → description only, `@param`/`@returns` excluded |
| `test_jsdoc_no_doc` | No JSDoc → `None` |
| `test_jsdoc_with_export` | JSDoc before `export function` → validates no panic |

### Integration Test

`test_run_ingestion_typescript`: Creates temp directory with `.ts`, `.tsx`, `.js` files, runs `run_ingestion()`, verifies all three files produce chunks with `language: "typescript"`, correct identifiers, and normalized paths.

### Unblocks

- V1.5: Docstring Extraction (wire `handler.extract_docstring()` into parser pipeline for Rust, Python, TypeScript)

**Crate:** code-raptor

---

## 2026-02-06: V1.3 Incremental Ingestion

### Summary
Implemented file-level incremental ingestion with three-layer architecture (parse → reconcile → orchestrate). Changed files are detected by SHA256 hash, unchanged files are skipped entirely. Includes schema tightening: `project_name` became non-optional, paths normalized to relative forward-slash format, CrateChunk hash fixed to include description. Chunk IDs switched from random UUID v4 to deterministic `hash(file_path, content)` for Track C edge stability. Content hashing normalizes CRLF → LF for cross-OS consistency.

### Architecture: Three-Layer Separation

```
Layer 1 (sync):  run_ingestion()         → IngestionResult (parse code, no DB)
Layer 2 (sync):  reconcile()             → ReconcileResult (data comparison, no DB)
Layer 3 (async): main.rs orchestration   → apply diff (DB reads/writes)
```

### Changes by Crate

| Crate | Changes |
|-------|---------|
| coderag-types | `project_name: Option<String>` → `String` on all types; `deterministic_chunk_id()` replaces random UUID; `content_hash()` normalizes CRLF |
| coderag-store | Arrow schemas nullable → non-nullable for project_name; added `get_file_index()`, `delete_chunks_by_ids()`, `get_embedding_model_version()` |
| code-raptor | New `reconcile` module; `resolve_project_name()` + `normalize_path()` helpers; orchestration in main.rs with `--full`, `--dry-run`, `--project-name` flags |
| portfolio-rag-chat | Updated context.rs, dto.rs, handlers, templates for non-optional project_name |

### New Module: `ingestion/reconcile.rs`

Pure data comparison — no I/O, no DB handle, fully unit-testable.

| Type | Purpose |
|------|---------|
| `ExistingFileIndex` | Per-table file → (hash, chunk_ids) mapping from DB |
| `ReconcileResult` | What to insert + what to delete + stats |
| `DeletionsByTable` | Deletions partitioned by LanceDB table |
| `IngestionStats` | Counts: unchanged, changed, new, deleted files + chunks |

| Function | Purpose |
|----------|---------|
| `reconcile()` | Entry point: compares current ingestion against existing index |
| `reconcile_by_file()` | Generic: many chunks per file (CodeChunk, ReadmeChunk) |
| `reconcile_single_per_file()` | Generic: 1:1 file mapping (ModuleDocChunk) |
| `reconcile_crates()` | By `crate_name` instead of file path |

### New VectorStore Methods

| Method | Purpose |
|--------|---------|
| `get_file_index(table, project, path_col)` | Returns file → (hash, chunk_ids) for change detection |
| `delete_chunks_by_ids(table, chunk_ids)` | Batch delete with `IN (...)` predicate, batched in groups of 100 |
| `get_embedding_model_version(project)` | Query one chunk's model version for mismatch detection |

### CLI Flags

| Flag | Behavior |
|------|----------|
| `--full` | Force full re-index: delete all project chunks → re-embed → re-insert |
| `--dry-run` | Run reconcile, print stats, no DB changes (conflicts with `--full`) |
| `--project-name <name>` | Override project name for all chunks (defaults to directory inference) |

### Incremental Flow

1. Parse code into chunks (sync, no DB)
2. Initialize embedder + store
3. Check embedding model version (mismatch → bail with `--full` suggestion)
4. Build existing index from DB (async)
5. Reconcile: pure data comparison (sync)
6. Insert new chunks first (safer on crash: duplicates > missing data)
7. Delete old chunks

### Schema Tightening

| Change | Before | After |
|--------|--------|-------|
| `project_name` | `Option<String>` | `String` (non-optional) |
| Path storage | Absolute, OS-specific | Relative to repo root, forward slashes |
| CrateChunk hash | Omitted description | `crate_name:description:deps` |
| CodeChunk hash | Per-chunk content hash | File-level SHA256 (all chunks from same file share hash) |
| `chunk_id` | Random UUID v4 | Deterministic `hash(file_path, content)` — stable across re-indexing |
| `content_hash()` | Raw bytes | CRLF-normalized before hashing (cross-OS consistency) |
| `resolve_project_name()` | N/A | CLI override > subdir name > repo dir name > "unknown" |

### Test Results

All 58 tests pass (0 warnings):
- coderag-types: 9 tests (deterministic ID + CRLF normalization tests added)
- coderag-store: 6 tests
- code-raptor: 26 unit + 9 integration tests (deterministic ID stability test added)
- portfolio-rag-chat: 8 tests

### Integration Tests (`tests/incremental_ingestion.rs`)

| Test | Verifies |
|------|----------|
| `roundtrip_no_changes` | Re-ingest same files → 0 inserts/deletes |
| `detects_modified_file` | Modified file → correct replacement, untouched files skipped |
| `detects_deleted_file` | Deleted file → chunks removed by ID |
| `detects_new_file` | New file → chunks inserted |
| `mixed_changes` | Changed + deleted + new + unchanged simultaneously |
| `project_name_override_stable_reconcile` | `--project-name` override produces stable reconcile |
| `paths_normalized` | All paths relative, forward slashes |
| `file_level_content_hash` | All chunks from same file share content hash |
| `deterministic_ids_stable_across_runs` | Same input produces identical chunk_ids across runs |

### Migration

Existing databases incompatible (schema change: nullable → non-nullable). Requires full re-ingestion:
```bash
rm -rf data/portfolio.lance
cargo run --bin code-raptor -- ingest /path/to/projects --db-path data/portfolio.lance
```

Subsequent ingestions are incremental by default:
```bash
cargo run --bin code-raptor -- ingest /path/to/projects --db-path data/portfolio.lance
cargo run --bin code-raptor -- ingest /path/to/projects --db-path data/portfolio.lance --dry-run
cargo run --bin code-raptor -- ingest /path/to/projects --db-path data/portfolio.lance --full
```

---

## 2026-02-06: V1.2 LanguageHandler Refactor

### Summary
Replaced monolithic `SupportedLanguage` enum with trait-based `LanguageHandler` abstraction. Adding a new language is now "implement one trait + register" instead of modifying 4+ match statements. Pure refactor — ingestion output identical before and after.

### Changes

| Change | Detail |
|--------|--------|
| New trait | `LanguageHandler` with `name()`, `extensions()`, `grammar()`, `query_string()`, `extract_docstring()` (default None) |
| Implementations | `RustHandler`, `PythonHandler` |
| Registry | `handler_for_path()`, `handler_by_name()`, `supported_extensions()` via `OnceLock<Vec<Box<dyn LanguageHandler>>>` |
| CodeAnalyzer | `analyze_content(src, lang)` → `analyze_file(path, src)` + `analyze_with_handler(src, handler)` |
| Module docs | `extract_module_docs()` uses `RustHandler` directly (Rust-specific `//!` syntax) |
| Deleted | `SupportedLanguage` enum entirely removed |

### New File Structure

```
crates/code-raptor/src/ingestion/
├── mod.rs              # Re-exports, orchestration
├── parser.rs           # CodeAnalyzer (updated)
├── reconcile.rs        # Reconcile module (V1.3)
├── language.rs         # LanguageHandler trait (new)
└── languages/
    ├── mod.rs          # Registry + handler_for_path (new)
    ├── rust.rs         # RustHandler (new)
    └── python.rs       # PythonHandler (new)
```

### Key Design Decisions

1. **Trait with default `extract_docstring`**: Returns `None` now, V1.4 implements per-handler
2. **`OnceLock` registry**: Zero-cost after first access, thread-safe
3. **`analyze_file()` as primary API**: Auto-detects language from path, cleaner call sites
4. **Rust-specific module docs**: `extract_module_docs()` uses `RustHandler` directly rather than generalizing

### Unblocks

- V1.4: TypeScript Support (implement `TypeScriptHandler` + register)
- V1.5: Docstring Extraction (wire `extract_docstring()` into parser pipeline)

**Crate:** code-raptor

---

## 2026-02-04: V1.1 Schema Foundation

### Summary
Added foundational schema fields and APIs required for incremental ingestion (V1.3) and call graph (Track C). All 4 chunk types now have `chunk_id`, `content_hash`, and `embedding_model_version` fields. Delete API added to VectorStore.

### Changes by Crate

| Crate | Changes |
|-------|---------|
| coderag-types | Added 3 fields to all chunk types + `content_hash()` and `new_chunk_id()` helpers |
| coderag-store | Updated Arrow schemas, batch conversions, changed deps to `List<Utf8>`, added delete API |
| code-raptor | Updated ingestion to populate new fields |
| portfolio-rag-chat | Updated test fixtures |

### New Fields (all chunk types)

| Field | Type | Purpose |
|-------|------|---------|
| `chunk_id` | String (UUID v4) | Stable foreign key for Track C call graph edges |
| `content_hash` | String (SHA256) | Change detection for incremental ingestion |
| `embedding_model_version` | String | Prevents silent embedding inconsistency |

### New Dependencies

```toml
# coderag-types
sha2 = "0.10"
uuid = { version = "1.20", features = ["v4"] }

# coderag-store
arrow-buffer = "56.2"
```

### Delete API (VectorStore)

| Method | Purpose |
|--------|---------|
| `delete_chunks_by_file(table, path)` | For incremental file updates |
| `delete_chunks_by_project(table, project)` | For project removal |
| `delete_chunk_by_id(table, chunk_id)` | For individual chunk deletion |
| `get_chunks_by_file(table, path)` | Returns `(chunk_id, content_hash)` pairs for comparison |

### Schema Change: Dependencies

`crate_chunks.dependencies` changed from CSV string to `List<Utf8>` Arrow type. Enables future "what depends on X?" queries.

### Test Results

All 34 tests pass:
- coderag-types: 5 tests (hash/UUID helpers)
- coderag-store: 6 tests (batch conversion)
- code-raptor: 15 tests (parsing/ingestion)
- portfolio-rag-chat: 8 tests (context building)

### Migration

Existing databases incompatible. Requires full re-ingestion:
```bash
rm -rf data/portfolio.lance
cargo run --bin code-raptor -- ingest /path/to/projects --db-path data/portfolio.lance
```

### Unblocks

- V1.3: Incremental Ingestion (uses `content_hash` for change detection)
- Track C: Call Graph (uses `chunk_id` for foreign key references)

---

## 2026-01-31: V0.3 Workspace Restructuring

### Summary
Restructured monolithic crate into a Cargo workspace with 3 subcrates. Separates concerns between indexing (code-raptor), storage (coderag-store), and shared types (coderag-types). Root crate becomes pure query interface consumer.

### Architecture

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
│  │           coderag-store                  │                │
│  │  - Embedder (FastEmbed)                 │                │
│  │  - VectorStore (LanceDB)                │                │
│  └─────────────────┬───────────────────────┘                │
│                    │                                        │
│                    ▼                                        │
│  ┌─────────────────────────────────────────┐                │
│  │           coderag-types                  │                │
│  │  - CodeChunk, ReadmeChunk               │                │
│  │  - CrateChunk, ModuleDocChunk           │                │
│  └─────────────────────────────────────────┘                │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

### New Crates

| Crate | Purpose |
|-------|---------|
| `crates/code-raptor/` | Ingestion CLI - parses repositories, extracts chunks, stores in LanceDB |
| `crates/coderag-store/` | Storage layer - Embedder (FastEmbed) + VectorStore (LanceDB) |
| `crates/coderag-types/` | Shared domain types - CodeChunk, ReadmeChunk, CrateChunk, ModuleDocChunk |

### Files

| File | Purpose |
|------|---------|
| `crates/code-raptor/src/main.rs` | CLI entry point with `ingest` and `status` commands |
| `crates/code-raptor/src/lib.rs` | Library exports for ingestion module |
| `crates/code-raptor/src/ingestion/mod.rs` | Directory walker, chunk extraction pipeline |
| `crates/code-raptor/src/ingestion/parser.rs` | CodeAnalyzer with tree-sitter AST queries |
| `crates/coderag-store/src/lib.rs` | Library exports |
| `crates/coderag-store/src/embedder.rs` | FastEmbed wrapper (BGE-small-en-v1.5, 384-dim) |
| `crates/coderag-store/src/vector_store.rs` | LanceDB 4-table schema, upsert/search operations |
| `crates/coderag-types/src/lib.rs` | CodeChunk, ReadmeChunk, CrateChunk, ModuleDocChunk structs |

### Key Design Decisions

1. **Workspace structure**: Enables independent compilation and clearer ownership boundaries
2. **code-raptor as standalone CLI**: Can run ingestion separately from query server
3. **Shared types crate**: Single source of truth for domain models across crates
4. **Store abstraction**: Both code-raptor and portfolio-rag-chat consume coderag-store

---

## 2026-01-01: V0.2 Docker Deployment

### Summary
Added Docker containerization for deployment. Two-stage workflow: first run ingestion container to populate LanceDB, then run query server container.

### Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    Docker Compose                           │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  Stage 1: Ingestion                                         │
│  ┌─────────────────────────────────────────┐                │
│  │  docker-compose-ingest.yaml             │                │
│  │  - Mounts source repos                  │                │
│  │  - Runs code-raptor ingest              │                │
│  │  - Outputs to shared LanceDB volume     │                │
│  └─────────────────────────────────────────┘                │
│                         │                                   │
│                         ▼                                   │
│               ┌─────────────────┐                           │
│               │  LanceDB Volume │                           │
│               └─────────────────┘                           │
│                         │                                   │
│                         ▼                                   │
│  Stage 2: Query Server                                      │
│  ┌─────────────────────────────────────────┐                │
│  │  docker-compose.yaml                    │                │
│  │  - Mounts LanceDB volume (read)         │                │
│  │  - Runs portfolio-rag-chat server       │                │
│  │  - Exposes port 3000                    │                │
│  └─────────────────────────────────────────┘                │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

### Files

| File | Purpose |
|------|---------|
| `Dockerfile` | Multi-stage build for Rust binary |
| `docker-compose.yaml` | Query server orchestration |
| `docker-compose-ingest.yaml` | Ingestion pipeline orchestration |
| `clean_docker.sh` | Cleanup script for containers/volumes |

### Key Design Decisions

1. **Two-stage workflow**: Separates expensive ingestion from lightweight query serving
2. **Shared volume**: LanceDB data persisted between containers
3. **Multi-stage Dockerfile**: Smaller final image, build dependencies not included

---

## 2025-12-23: V0.1 MVP - Core Engine Functional

### Summary
Implemented complete RAG chatbot MVP for code repositories. Parses Rust/Python codebases with tree-sitter, generates embeddings with FastEmbed, stores in LanceDB, and answers questions via Google Gemini. Web UI built with htmx + Askama.

### Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                     Ingestion Pipeline                      │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  Repository Files                                           │
│       │                                                     │
│       ▼                                                     │
│  ┌─────────────────┐                                        │
│  │  CodeAnalyzer   │  tree-sitter AST parsing               │
│  │  (parser.rs)    │  Rust: function_item, struct_item, ... │
│  └────────┬────────┘  Python: function_definition, class_...│
│           │                                                 │
│           ▼                                                 │
│  ┌─────────────────┐                                        │
│  │ Chunk Extraction│  CodeChunk, ReadmeChunk,               │
│  │  (ingestion/)   │  CrateChunk, ModuleDocChunk            │
│  └────────┬────────┘                                        │
│           │                                                 │
│           ▼                                                 │
│  ┌─────────────────┐                                        │
│  │    Embedder     │  FastEmbed BGE-small-en-v1.5           │
│  │  (embedder.rs)  │  384-dimensional vectors               │
│  └────────┬────────┘                                        │
│           │                                                 │
│           ▼                                                 │
│  ┌─────────────────┐                                        │
│  │   VectorStore   │  LanceDB with 4 tables:                │
│  │ (vector_store)  │  code_chunks, readme_chunks,           │
│  └─────────────────┘  crate_chunks, module_doc_chunks       │
│                                                             │
└─────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────┐
│                      Query Pipeline                         │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  User Query                                                 │
│       │                                                     │
│       ▼                                                     │
│  ┌─────────────────┐                                        │
│  │   Axum Router   │  GET /, POST /api/chat, POST /chat     │
│  │    (api/)       │  GET /projects, GET /health            │
│  └────────┬────────┘                                        │
│           │                                                 │
│           ▼                                                 │
│  ┌─────────────────┐                                        │
│  │    Retriever    │  Embeds query → searches 4 tables      │
│  │  (retriever.rs) │  Returns RetrievalResult               │
│  └────────┬────────┘                                        │
│           │                                                 │
│           ▼                                                 │
│  ┌─────────────────┐                                        │
│  │ Context Builder │  Formats chunks into markdown          │
│  │  (context.rs)   │  Builds system + user prompt           │
│  └────────┬────────┘                                        │
│           │                                                 │
│           ▼                                                 │
│  ┌─────────────────┐                                        │
│  │    Generator    │  Google Gemini via rig-core            │
│  │  (generator.rs) │  Returns answer + sources              │
│  └────────┬────────┘                                        │
│           │                                                 │
│           ▼                                                 │
│  ┌─────────────────┐                                        │
│  │   Web Response  │  htmx partial (HTML) or JSON           │
│  │    (web.rs)     │  Askama templates                      │
│  └─────────────────┘                                        │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

### Files

**API Layer**
| File | Purpose |
|------|---------|
| `src/api/mod.rs` | Router setup with all endpoints |
| `src/api/handlers.rs` | Request handlers for chat, projects, health |
| `src/api/state.rs` | AppState with Embedder, VectorStore, LlmClient |
| `src/api/dto.rs` | ChatRequest, ChatResponse DTOs |
| `src/api/error.rs` | Error types and responses |
| `src/api/web.rs` | Askama templates, htmx integration |

**Engine Layer**
| File | Purpose |
|------|---------|
| `src/engine/mod.rs` | Engine module exports |
| `src/engine/retriever.rs` | Vector search across 4 tables |
| `src/engine/context.rs` | Prompt building, chunk formatting |
| `src/engine/generator.rs` | LLM response generation |
| `src/engine/config.rs` | RetrievalConfig, EngineConfig |

**Entry Point**
| File | Purpose |
|------|---------|
| `src/main.rs` | Server startup, environment loading |

### Tech Stack

| Component | Technology |
|-----------|------------|
| Web Framework | Axum 0.8 |
| LLM | Google Gemini (rig-core 0.27) |
| Vector Database | LanceDB |
| Embeddings | FastEmbed (BGE-small-en-v1.5, 384-dim) |
| Code Parsing | tree-sitter (Rust, Python) |
| Frontend | htmx + Askama templates |
| Async Runtime | Tokio 1.48 |

### Retrieval Configuration

| Chunk Type | Default Limit |
|------------|---------------|
| Code | 5 |
| README | 2 |
| Crate | 3 |
| Module Docs | 3 |

### Key Design Decisions

1. **Function-level chunking**: 1 function/class → 1 vector for precise retrieval
2. **4-table schema**: Separate tables for different content types with specialized formatting
3. **htmx frontend**: Server-rendered HTML with async updates, minimal JS
4. **Mutex on Embedder**: Only resource needing synchronization (model weights)
5. **rig-core for LLM**: Clean abstraction over Gemini API

### Known Limitations (documented for future work)

- `docstring` field exists but always `None` (extraction not implemented)
- No call graph or cross-function relationships
- No incremental ingestion (full re-scan each time)
- No hybrid search (semantic only, no BM25)
