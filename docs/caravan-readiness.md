# Caravan SDK Thesis & code-rag Readiness Evaluation

An analysis of how structurally compatible code-rag's current architecture is with the (currently unimplemented) [Caravan](https://github.com/paulxiep/caravan) SDK pattern — i.e., if Caravan were implemented tomorrow, how much of code-rag would need to change to deploy across packaging/placement permutations through Caravan's SDK seams?

Scope: evaluation only, treating the Caravan SDK as hypothetical. Four candidate seams under review — Embedder, Reranker, VectorStore, Ingestion (code-raptor).

---

## 1. Caravan's Thesis in One Paragraph

Caravan is a **backend** application-definition compiler. The same source code deploys across three **orthogonal** axes — *packaging* (monolith / multi-container / multi-service), *placement* (local docker-compose / Fargate / Lambda / batch), and *composition* (local OSS engine / cloud managed service / referenced existing resource) — by reading a single `caravan.yaml`. The compiler emits HCL (Terraform) and Compose projections of an IR. The structural contract user code must obey is the `caravan-rpc-<lang>` SDK at inter-component seams: a call site written as `client::<Interface>().method()` dispatches as **inproc / HTTP / Lambda** based on a compiler-injected `CARAVAN_RPC_PEERS` table. **Frontend deployment — bundles, hosting, browser-sandbox concerns — is explicitly outside Caravan's scope.** FE is inherently monolithic from Caravan's vantage; Caravan makes no claims about it.

**State today:** thesis + evidence catalogs + PoC specs. No CLI, no SDK packages, no compiler. The Caravan stub CLI prints "not implemented yet" and points users to docs.

---

## 2. Two Clarifications Before Evaluating

**Clarification A — FE vs BE.** code-rag's workspace contains *both* a frontend (Leptos → browser WASM, see `code-rag-ui`) and backend surfaces (Axum HTTP server, MCP stdio server, code-raptor CLI). **Caravan's scope covers only the backend.** The frontend's existence — including the unusual `standalone` mode that embeds the full pipeline for client-side retrieval — does not enter into Caravan readiness; it's a separate problem with a separate (and already monolithic-by-nature) deployment story.

**Clarification B — IO shell ≠ component split.** Among the backend surfaces, code-rag's existing "multimodal" means *different IO shells wrapping one in-process pipeline* (HTTP handler vs. MCP tool handler vs. CLI invocation, all calling the same `retriever::retrieve()`). Caravan's "multimodal" means *one IO shell with internal components split across processes/hosts*. These are **orthogonal** — solving one doesn't solve the other, but they don't conflict either.

| Surface | Kind | In Caravan's blast radius? |
|---|---|---|
| code-rag-ui (Leptos CSR / browser WASM) | **Frontend** | **No — out of scope by design** |
| code-rag-chat (Axum HTTP server) | Backend daemon | Yes — fits Caravan's `http` entry kind |
| code-rag-mcp (MCP stdio server) | Backend daemon | Conceptually yes; PoC doesn't enumerate stdio entry kind, but the process *is* a backend daemon and could be added as an entry kind without architectural friction |
| code-raptor (ingestion CLI) | Backend batch | Yes — fits Caravan's `batch`/`cron` entry kind |

---

## 3. code-rag Architecture, with FE/BE Boundary Made Explicit

```
FRONTEND (outside Caravan scope; inherently monolithic):
  code-rag-ui          — Leptos CSR; standalone feature -> WASM bundle that
                         embeds engine + store for client-side retrieval

BACKEND (Caravan's domain):
  Entries (deployable processes):
    code-rag-chat      — Axum HTTP server  [entry: http]
    code-rag-mcp       — MCP stdio server  [entry: stdio - conceptual]
    code-raptor        — Ingestion CLI     [entry: batch / cron]

  Components (would be seam providers under Caravan):
    code-rag-engine    — pure orchestration (intent/retriever/graph/fusion)
    code-rag-store     — Embedder + Reranker + VectorStore impls

  Shared library:
    code-rag-types     — types only, no logic
```

The load-bearing BE struct is [`AppState`](../src/api/state.rs):

```rust
pub struct AppState {
    pub embedder: Mutex<Embedder>,
    pub reranker: Option<Mutex<Reranker>>,
    pub classifier: IntentClassifier,
    pub store: VectorStore,
    pub llm: LlmClient,
    pub config: EngineConfig,
}
```

All four candidate seams are fields here. They are concrete struct types invoked through direct method calls — no `trait` abstraction at the seams today.

**Mapping onto Caravan concepts:**

- **Entries:** Axum HTTP server (`http`), code-raptor (`batch`/`cron`), and conceptually MCP stdio (would need a new entry kind).
- **Seams:** `Embedder`, `Reranker`, `VectorReader`, `VectorWriter`, `LlmClient`.
- **Resources:** vector search ↔ Caravan `search` group (LanceDB-local / Pinecone / Qdrant / etc.); call-graph edges ↔ Caravan `db.sql` (or similar) group — *currently bundled into the same LanceDB store, but conceptually separate*; `LlmClient` ↔ Caravan `llm` group.

---

## 4. Seam-by-Seam Readiness Evaluation

For each seam: today's coupling, the natural Caravan interface shape, and practical viability of splitting it.

### 4.1 Embedder ([crates/code-rag-store/src/embedder.rs](../crates/code-rag-store/src/embedder.rs))

- **Today's coupling:** concrete `Embedder` struct wrapping FastEmbed BGE-small. Called via `state.embedder.lock().embed(text)` from the retriever entry path and from code-raptor's ingest loop. Behind `Mutex` because FastEmbed is `!Sync`.
- **Caravan-fit shape:** Trivial. Natural interface is `embed(texts: Vec<String>) -> Vec<Vec<f32>>`. Pure function semantics, deterministic, no side effects, no shared state across calls. Maps cleanly to `inproc | container | lambda` dispatch — this is the textbook Caravan seam.
- **Practical viability:** Strong. ~50 MB ONNX model warms slowly; sharing one container across query and ingest workloads reduces per-process memory. Lambda makes less sense (cold-start vs. model load); container is the natural cloud target. Local dev stays inproc.
- **Readiness verdict: HIGH.** One refactor — promote `Embedder` to a trait, owners hold `Box<dyn EmbedderApi>` — and the seam is Caravan-ready. The `Mutex` disappears in the HTTP/Lambda case (request-scoped) and survives in the inproc case.

### 4.2 Reranker ([crates/code-rag-store/src/reranker.rs](../crates/code-rag-store/src/reranker.rs))

- **Today's coupling:** `Option<Mutex<Reranker>>` on AppState — already optional, since MCP and the lighter retrieval paths may skip it. Called from inside the engine's per-intent reranking gate.
- **Caravan-fit shape:** Very clean — `rerank(query: String, candidates: Vec<Doc>) -> Vec<f32>`. Pure scoring function over inputs. Equally well-suited to inproc / container / Lambda.
- **Practical viability:** Strongest case of all four. The ms-marco-MiniLM ONNX model is the single heaviest in-process artifact (~88 MB), and reranking is called per-query but **only for some intents** — so isolating it lets the rest of the app stay light and lets the reranker scale independently. The ms-marco-MiniLM-L-6-v2 constraint (required for browser parity) is orthogonal — it's a model choice, not an architectural one, and survives any packaging decision.
- **Readiness verdict: HIGH.** Same refactor as Embedder. The already-`Option<>` shape on AppState is a structural hint that this component is the most logically separable today.

### 4.3 VectorStore ([crates/code-rag-store/src/vector_store.rs](../crates/code-rag-store/src/vector_store.rs))

- **Today's coupling:** Owns the LanceDB connection. Provides vector search (cosine + FTS hybrid), graph-edge lookups (`call_edges` scalar table), and ingest-side write paths. Called from the engine's retrieve and graph-augmentation paths; also called from code-raptor for writes.
- **Caravan-fit shape — vector search itself:** Clean. From the engine's vantage, `VectorReader.search(query) -> ranked docs` doesn't care whether the implementation is LanceDB-local, Pinecone-managed, Qdrant-self-hosted, or any other backend in Caravan's `search` resource group. **Swapping LanceDB → Pinecone is the textbook Caravan *composition* operation:** the resource adapter changes, the seam interface doesn't. Hybrid search differs in mechanics (LanceDB's FTS index vs. Pinecone's sparse-dense vectors), but that's an implementation concern internal to one resource adapter, not an architectural obstacle.
- **The real wrinkle — bundled storage concerns.** The current `VectorStore` struct fuses *two* logically separate resources: (1) vector search, and (2) the `call_edges` graph-edges scalar table, which has no vector-similarity component and is closer to a `db.sql`/`kv` resource. LanceDB happens to serve both, which made the implementation convenient but is a category conflation under Caravan's resource model. Pinecone wouldn't host `call_edges` (it's not a vector workload); under Caravan these would naturally split into two independently-composable resources.
- **Bimodal read/write API.** Read path (queries) and write path (ingest) have different consistency/locking concerns. Caravan's RPC contract is HTTP request/response; fine for reads, fine for batched writes. Splitting the seam into `VectorReader` and `VectorWriter` traits is a small refactor and resolves the question of who owns the write lock (the writer-side process does).
- **Practical viability:** Strong. Vector search is among the most cleanly composable resources in the system; managed-vector-DB options are mature.
- **Readiness verdict: HIGH (with one teardown).** Stronger than I initially rated. The teardown is: inside `code-rag-store`, separate the vector-search resource from the call-graph-edges resource so each can be composition-swapped independently. Then add `VectorReader` / `VectorWriter` trait split. After that, LanceDB ↔ Pinecone is a yaml-line change in `composition:`.

### 4.4 Ingestion / code-raptor ([crates/code-raptor/src/main.rs](../crates/code-raptor/src/main.rs))

- **Today's coupling:** Already a separate binary. Subcommands `ingest`, `status`, `export`. Uses tree-sitter parsers + Embedder + VectorStore (write path). Runs on demand, not as a service.
- **Caravan-fit shape:** **Most natural fit of all four — but for a different reason.** code-raptor isn't a *seam* in the Caravan sense (interface boundary between components); it's an **entry kind** in the Caravan yaml sense — a `batch` or `cron` entry root. It would deploy as `placement: batch` (cloud batch job) or stay local. Its *internal* SDK use would be to call Embedder and VectorWriter through the dispatch table.
- **Practical viability:** Strong, and conceptually obvious — ingestion is by nature lumpy/batch work, perfect for cloud batch or Lambda-with-large-timeout. The SHA256 incremental logic already exists.
- **Readiness verdict: HIGH.** The component-vs-entry distinction is what makes it the cleanest case: it doesn't need to *expose* a Caravan interface, it just needs to *consume* the Embedder and VectorWriter ones. No refactor required at the code-raptor layer once 4.1 and 4.3 are in shape.

---

## 5. Architectural Strengths (Already SDK-Friendly)

These are properties code-rag already has that would make Caravan adoption mechanical rather than architectural:

- **Engine purity was earned via a stricter constraint than Caravan requires.** [code-rag-engine](../crates/code-rag-engine/src/lib.rs) has zero I/O and compiles to both native and `wasm32-unknown-unknown` because the **browser-WASM frontend** embeds it for client-side retrieval. **Browser sandbox is strictly tighter than Caravan's RPC boundary** — anything that works in WASM trivially works behind a Caravan dispatch table. Caravan's "pure call sites in the engine" requirement is *already satisfied as a side-effect of FE work code-rag had to do anyway*.
- **Workspace crate boundaries match Caravan's "component" concept.** code-rag-engine / code-rag-store / code-raptor map cleanly to Caravan modules.
- **Optional components precedent.** `Option<Reranker>` and MCP-skips-LLM demonstrate the codebase already tolerates absent seams — the pattern of "this seam may or may not be wired up" exists.
- **Deterministic, read-mostly steady state.** After ingest, the system is read-only at the index level. No consensus, no distributed write coordination, no cache-coherence concerns. This is friendlier to lambda/container splits than typical applications.
- **Two BE entry kinds already shipping** (Axum HTTP, code-raptor batch) — the multi-entry pattern Caravan assumes is already real in code-rag.

---

## 6. Architectural Gaps (What Would Need to Change)

These are the concrete obstacles to Caravan dispatch over code-rag's seams:

- **No trait boundaries at the seams.** Embedder/Reranker/VectorStore are concrete structs. Caravan's `@interface ... -> impl Trait` model needs a Rust `trait` per seam. None exists today.
- **AppState is a god-struct holding concrete components.** [src/api/state.rs](../src/api/state.rs) directly owns each component by concrete type. Under Caravan, callers should hold `dyn Trait` (or generics) so the SDK can swap inproc/HTTP implementations.
- **code-rag-mcp depends on code-rag-chat for AppState.** Pulls in Axum/rig-core/tokio that MCP doesn't strictly need. The standard fix (extract a `code-rag-core` crate holding `AppState`) is also a prerequisite for Caravan: that core is where the trait-typed seams would live.
- **VectorStore conflates three things.** Per §4.3 — (a) reader + writer in one struct, (b) vector-search and call-graph-edges as one resource. The interface split (`VectorReader`/`VectorWriter`) and the resource split (search resource vs. graph-edges resource) are independent, mechanical refactors.
- **No interface registration / dispatch awareness.** Code calls `state.embedder.embed(...)` directly. Under Caravan that becomes `client::<Embedder>().embed(...)`, where `client::<>` consults `CARAVAN_RPC_PEERS`. No such indirection layer exists today. Adding it is mechanical once the traits are in place.

Note: **the frontend is not listed as a gap.** code-rag-ui is out of Caravan's scope by design — its packaging is independent of Caravan's three-dimensional matrix, so its existence is neither a readiness blocker nor a strength to claim credit for.

---

## 7. Overall Readiness Verdict

**Readiness rating: HIGH. Roughly 80% structurally.**

The rating rests on one realization: code-rag-engine's I/O-free purity was already paid for to satisfy the browser-WASM frontend, which is a strictly tighter constraint than Caravan imposes. Caravan readiness is therefore a *byproduct* of FE work code-rag had to do anyway, not a separate investment. The remaining gaps are all mechanical and well-localized.

Distilled:
- The *expensive* architectural work is done: pure engine, crate-level component boundaries, no I/O leakage into algorithms, multi-entry-kind precedent (HTTP + batch).
- Remaining work is mechanical and well-localized: introduce traits at the four seams, extract `code-rag-core`, split `VectorStore` along two axes (reader/writer + search-resource/graph-edges-resource), and replace direct `state.x.method()` calls with `client::<X>().method()` once the SDK exists.
- One genuine architectural decision still open: **whether MCP-stdio gets adopted as a Caravan entry kind or stays a separate deployment surface** (it's conceptually compatible but not in the current PoC). The previously-flagged LanceDB-vs-managed-vector-DB question is *not* an architectural decision — it's exactly what Caravan's *composition* axis is built to handle, a yaml-line change once the resource adapters exist.

**code-rag would not have to be redesigned to adopt Caravan's SDK. It would need to be refactored — roughly one well-defined PR per seam, plus one core-extraction PR.** The thesis-level fit is good and the FE-driven engine purity gives it a free head start on Caravan readiness.

**Caveat.** Caravan is pre-implementation. The PoC RPC spec is the most likely shape, but until the SDK exists in Rust, any concrete preparation in code-rag would be premature. The right time to act on this evaluation is when `caravan-rpc-rust` ships v0.1 — at which point this document becomes the input to a focused refactor plan.

---

## 8. Key Reference Files

**code-rag — backend (Caravan-relevant):**
- [src/api/state.rs](../src/api/state.rs) — AppState, the god-struct
- [crates/code-rag-engine/src/lib.rs](../crates/code-rag-engine/src/lib.rs) — pure engine, would host Caravan call sites
- [crates/code-rag-store/src/embedder.rs](../crates/code-rag-store/src/embedder.rs) — seam 4.1
- [crates/code-rag-store/src/reranker.rs](../crates/code-rag-store/src/reranker.rs) — seam 4.2
- [crates/code-rag-store/src/vector_store.rs](../crates/code-rag-store/src/vector_store.rs) — seam 4.3 (the awkward one)
- [crates/code-raptor/src/main.rs](../crates/code-raptor/src/main.rs) — seam 4.4 (entry, not interface)
- [crates/code-rag-mcp/Cargo.toml](../crates/code-rag-mcp/Cargo.toml) — the chat-dep weak spot

**code-rag — frontend (out of Caravan scope, listed for completeness):**
- `crates/code-rag-ui/` — Leptos CSR + WASM standalone mode (embeds the BE pipeline client-side; FE bundling and hosting concerns are not Caravan's problem)

**Caravan (sibling repo, `../caravan/`):**
- `thesis.md` — three orthogonal dimensions, source-unchanged principle
- `docs/poc_rpc_sdk.md` — SDK surface, wire contract, dispatch table
- `docs/poc_yaml_spec.md` — yaml schema, entries vs seams
- `docs/poc_groups_to_code.md` — 10 resource groups, local↔cloud swaps
- `cmd/caravan/main.go` — stub CLI, current state proof
