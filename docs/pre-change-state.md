# code-rag — Pre-Change State

Snapshot of code-rag immediately before any Caravan-conversion changes begin on the `caravan-conversion` branch. Reference point for verifying that conversion work doesn't regress any of the five pre-existing deployment surfaces.

## Repo and branch

- **Repo**: `code-rag`
- **Branch**: `caravan-conversion`
- **Baseline commit**: `d5d24fa270bf6045d3be22159e47b54a03df3312`
- **Conversion target**: introduce Caravan SDK seams while preserving every pre-existing deployment surface (B0p in `caravan/docs/development_plan.md`).

## What this doc is

The "what existed before" reference for Caravan conversion. If a verification command below fails after a conversion-related commit, that conversion regressed something. The doc gets updated only when an invariant is intentionally retired (and that decision must be noted in `development_plan.md` first).

## Code structure

6-crate Cargo workspace. Detailed per-crate breakdown in [caravan-readiness.md](caravan-readiness.md) §3.

```
code-rag/
├── Cargo.toml                              (workspace root)
├── src/                                    (code-rag-chat crate — Axum HTTP server)
│   ├── main.rs                             (chat server entry)
│   ├── bin/harness.rs                      (quality eval binary)
│   ├── api/state.rs                        (AppState — god-struct, holds all seams)
│   ├── api/handlers.rs                     (HTTP route handlers)
│   └── api/mod.rs                          (router + middleware)
├── crates/
│   ├── code-rag-types/                     (pure types, no logic)
│   ├── code-rag-engine/                    (pure orchestration, wasm32-compilable)
│   ├── code-rag-store/                     (Embedder + Reranker + VectorStore + LlmClient)
│   │   ├── src/embedder.rs                 ← seam candidate (FastEmbed BGE-small)
│   │   ├── src/reranker.rs                 ← seam candidate (FastEmbed ms-marco-MiniLM)
│   │   ├── src/vector_store.rs             ← seam candidate (LanceDB; needs splitting)
│   │   └── src/llm.rs                      ← seam candidate (Gemini via rig-core)
│   ├── code-raptor/                        (ingest CLI; entry: batch/cron)
│   ├── code-rag-ui/                        (Leptos WASM frontend; OUT OF CARAVAN SCOPE)
│   └── code-rag-mcp/                       (MCP stdio server; single-binary entry)
├── dockerfile/
│   └── Dockerfile                          (multi-stage; targets: chat, raptor)
├── docker-compose.yaml                     (chat server profile)
├── docker-compose-ingest.yaml              (one-shot ingest profile)
├── .github/workflows/
│   ├── build-check.yml                     (CI)
│   ├── gh-pages.yml                        (WASM standalone deploy)
│   └── release.yml                         (cross-platform binary for code-rag-mcp)
├── docs/
│   ├── caravan-readiness.md                (seam-by-seam evaluation — HIGH ~80%)
│   ├── pre-change-state.md                 (this file)
│   ├── executive_summary.md
│   ├── technical_summary.md
│   └── release.md
└── development_log.md
```

## Existing deployment surfaces

Each surface is a distinct way the codebase runs today. Caravan conversion must preserve all five.

### 1. Docker compose, chat target

**Invocation:** `docker-compose up --build`

**What runs:** `code-rag-chat` Axum HTTP server bound to localhost:3000. Persists LanceDB to `./data/portfolio.lance`, FastEmbed model cache to `./cache/`.

**Build context:** parent directory of the code-rag repo (`context: ..`), copying from `code-rag/src/...`. Non-standard; intentional so the Dockerfile can access sibling project sources for ingestion (see surface #2).

**Persistence requirements:** `./data/` (writable), `./cache/` (writable), `.env` with `GEMINI_API_KEY`.

### 2. Docker compose, ingest target

**Invocation:** `docker-compose -f docker-compose-ingest.yaml up --build`

**What runs:** `code-raptor ingest /repos --db-path /app/data/portfolio.lance`. Mounts the parent directory (all sibling projects) read-only at `/repos` and walks each subdirectory as a project. One-shot — exits when ingestion completes.

**Build context:** parent of repo, same Dockerfile, target `raptor`.

**Persistence requirements:** `./data/` (writable; LanceDB written here), `./cache/`.

### 3. Local non-container run

**Invocations:**
```bash
cargo run --release -p code-rag-chat              # Axum server
cargo run --release -p code-raptor -- ingest /path/to/repos  # ingest CLI
cargo run --release --bin harness                 # quality harness
cargo run --release -p code-rag-mcp               # MCP stdio server
```

**Persistence:** `DB_PATH`, `FASTEMBED_CACHE_DIR`, `CODE_RAG_RERANKER_DIR` env vars point at local paths. `.env` carries `GEMINI_API_KEY`.

### 4. GitHub Actions: gh-pages deploy

**Workflow:** [.github/workflows/gh-pages.yml](../.github/workflows/gh-pages.yml). Manually triggered (`workflow_dispatch`).

**What runs:** Clones configured target repos, runs `cargo run -p code-raptor -- ingest ... --db-path /tmp/demo.lance`, exports static JSON via `code-raptor export`, builds WASM via `trunk build --release --features standalone`, deploys `crates/code-rag-ui/dist/` to GitHub Pages.

**Runtime:** Browser-only. Engine + store compile to `wasm32-unknown-unknown` and run client-side from the static bundle. No backend.

**Caravan implication:** the engine must remain wasm32-compilable. Caravan SDK calls in `code-rag-engine` must not pull tokio or non-wasm-compatible dependencies into the pure path.

### 5. GitHub Actions: release

**Workflow:** [.github/workflows/release.yml](../.github/workflows/release.yml). Manually triggered with version input.

**What runs:** Matrix build of `code-rag-mcp` across `x86_64-unknown-linux-gnu`, `aarch64-apple-darwin`, `x86_64-pc-windows-msvc`. Each artifact: binary + `code-rag-mcp.config.yaml` template + LICENSE, zipped, attached to a draft GitHub Release.

**End-user install path:** Download zip → extract → edit `code-rag-mcp.config.yaml` → double-click binary. Binary writes `.claude/skills/code-rag.md`, `.mcp.json`, and `.gitignore` entry into the target dir; exits. Then Claude Code spawns it as an MCP stdio server.

**Caravan implication:** `code-rag-mcp` is a single-binary entry kind with zero runtime dependencies (no Postgres, no Redis, no shared services). Caravan must allow this entry to remain independently buildable and to ship as a single binary with no Caravan runtime requirements when run standalone.

## Seam candidates and resource bindings

Cross-reference: [caravan-readiness.md](caravan-readiness.md) §4 (full per-seam evaluation, HIGH ~80% verdict).

### Seams (to be declared via `caravan-rpc` SDK)
- **Embedder** — `crates/code-rag-store/src/embedder.rs`. FastEmbed BGE-small. Pure function over text inputs.
- **Reranker** — `crates/code-rag-store/src/reranker.rs`. FastEmbed ms-marco-MiniLM-L-6-v2 cross-encoder. Optional (gated by `ENABLE_RERANKER`).
- **VectorReader** + **VectorWriter** (split from current `VectorStore`) — `crates/code-rag-store/src/vector_store.rs`. LanceDB.
- **LlmClient** — `src/engine/generator.rs`. Gemini via `rig-core`.

### Resources (to be declared in caravan.yaml)
- **search** group: LanceDB-local (current) or managed vector DB (Pinecone, Qdrant) in future.
- **db.sql** or **kv** group: `call_edges` table — currently bundled in LanceDB but conceptually separate (per readiness §4.3).
- **llm** group: Gemini (current) or Bedrock for cloud composition (Tier-1 hard-pair).

### Entry kinds
- `http` (code-rag-chat / Axum server) — Caravan-native.
- `batch` or `cron` (code-raptor) — Caravan-native.
- `stdio` (code-rag-mcp) — would need a new Caravan entry kind, or stay outside Caravan and just consume the SDK as a client.

### Out of Caravan scope
- `crates/code-rag-ui/` — Leptos WASM frontend. FE explicitly out of scope per thesis. Listed here only to flag that Caravan must not break the WASM compile of `code-rag-engine`.

## Invariants to preserve

Caravan-conversion commits must not regress any of these:

1. **`cargo check --workspace` succeeds.**
2. **`cargo test --workspace` passes** (or whatever subset was passing on the baseline commit).
3. **`trunk build --release --features standalone --public-url /code-rag/ --dist /tmp/dist crates/code-rag-ui/index.html` succeeds** — WASM standalone build for gh-pages.
4. **`cargo build --release -p code-rag-mcp`** produces a single-binary that runs `code_rag_reindex` and the MCP setup flow without requiring any other process (Postgres, Redis, etc.).
5. **`docker-compose up --build`** brings up the chat server; `curl localhost:3000` returns the UI.
6. **`docker-compose -f docker-compose-ingest.yaml up --build`** completes ingestion against `/repos` mount.
7. **No-config inertness** — SDK-wrapped code runs with no `CARAVAN_RPC_PEERS` set, identical to pre-conversion behavior. This is the load-bearing acceptance for B0p.

## Verification commands

Run on the baseline commit `d5d24fa270bf6045d3be22159e47b54a03df3312` (and after any conversion commit):

```bash
# 1. Workspace builds.
cargo check --workspace

# 2. Tests pass.
cargo test --workspace

# 3. WASM build (gh-pages surface).
trunk build --release --features standalone --public-url /code-rag/ --dist /tmp/dist crates/code-rag-ui/index.html

# 4. Single-binary MCP build (release surface).
cargo build --release -p code-rag-mcp

# 5. Docker compose chat target builds (smoke check — full up requires .env).
docker-compose build

# 6. Docker compose ingest target builds.
docker-compose -f docker-compose-ingest.yaml build

# 7. Single-binary smoke (release surface end-to-end).
./target/release/code-rag-mcp --help
```

After Caravan conversion (B0p onward), additionally:

```bash
# 8. No-config inertness check — local-run with no env var, no docker.
cargo run --release -p code-rag-chat -- --health
# Should respond OK with the SDK present but unconfigured.
```
