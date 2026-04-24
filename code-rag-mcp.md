# Feasibility: code-rag as an MCP + Skill layer for Claude Code (single-repo)

## Context

Evaluating whether this code-rag project — a Rust RAG pipeline with intent classification, hybrid BM25+vector search, cross-encoder reranking, and a call-graph — could plug into Claude Code as an **MCP server** (exposing retrieval tools) paired with a **Claude Code Skill** (instructions telling Claude *when* to reach for them). The use case is **single-repo, inner-loop**: code-rag would supplement (or partially displace) Claude Code's native Grep/Glob on the repo it's currently working in. This write-up assesses fit, not implementation detail.

---

## Verdict

**Yes, it's feasible and differentiated — but as a complement to Grep/Glob, not a replacement.** The honest framing is: Grep/Glob are zero-setup, always-fresh, and unbeatable for exact-string lookups. Code-rag's edge is **semantic, intent-classified, relationship-aware** retrieval — things Grep structurally cannot do. A Skill file that routes *conceptual/architectural/relationship* queries to code-rag and leaves exact-symbol lookups to Grep is the natural shape.

The main risks are **index freshness** during an active coding session and **ingest setup friction** — not the protocol or query-quality side.

---

## What already exists that makes this easy

The shared [code-rag-engine](crates/code-rag-engine/) crate is pure algorithms (native + wasm32), with no I/O — so the same retrieval brain already runs in two surfaces (Axum server + in-browser WASM). Adding a third surface (MCP) is the same pattern: thin binary, call into the engine. Specifically:

- **Server-side retrieval entrypoint** at [src/engine/retriever.rs](src/engine/retriever.rs) via `retriever::retrieve(QueryContext, store, embedder, config, global_config, reranker)` — already drives the full intent → ArmPolicy → graph → rerank pipeline. Called from [src/api/handlers.rs:42](src/api/handlers.rs#L42).
- **Retrieval-only path** already exists on the WASM side: [crates/code-rag-ui/src/standalone_api.rs:41](crates/code-rag-ui/src/standalone_api.rs#L41) (`send_chat_rag_only`) returns sources without invoking the LLM. The server path doesn't have a ready-made LLM-less wrapper but `retriever::retrieve` returns the full `RetrievalResult` before generation — trivially extractable.
- **Intent classification + routing** in `code_rag_engine::intent` — `pre_classify_comparison`, `classify`, `route`, `arm_policy`. Already shared between server and WASM.
- **Graph surface** in `code_rag_engine::graph` — `detect_direction`, `graph_augment`, `reserve_graph_slots`. Call edges live in LanceDB scalar-only `call_edges` table (~3011 edges on this repo).
- **Source shape** ready to marshal: [src/api/dto.rs:22](src/api/dto.rs#L22) `SourceInfo { chunk_type, path, label, project, relevance, line }` — already the right schema for an MCP tool response (file/line/excerpt + score). `RetrievalResult::flatten()` sorts cross-type by relevance.
- **Ingestion** via [code-raptor](crates/code-raptor/) CLI with incremental SHA256 skip — re-ingest after edits is cheap.

The existing workspace binaries ([Cargo.toml:11-18](Cargo.toml#L11-L18)): `code-rag-chat` (HTTP server) and `code-rag-harness` (eval). Adding a `code-rag-mcp` binary sits alongside them and reuses the same `AppState` shape (store + embedder + reranker + classifier).

---

## What doesn't fit cleanly

1. **Single-repo ingest mode.** Per [README.md:10-20](README.md#L10-L20), ingestion assumes a *parent folder with sibling projects*. An inner-loop Claude Code user would expect `code-rag-ingest .` against the current repo. This is probably a small flag in [crates/code-raptor/src/main.rs](crates/code-raptor/src/main.rs) but needs verification — the "project name" assumption is baked into chunk paths (parent dir is repo root; file paths are project-name-prefixed).

2. **Index staleness during a session.** Claude Code edits files live; the LanceDB index only reflects the last ingest. Incremental re-ingest is fast but has to be *triggered*. Options: (a) Skill tells Claude to re-run ingest on changed files before a code-rag query; (b) MCP server auto-runs incremental ingest on each query (latency hit); (c) accept staleness and advise Grep for just-edited code. (c) is the honest default for a feasibility pass.

3. **Embedder / reranker cold-start.** Long-lived MCP server amortizes load (BGE-small + ms-marco-MiniLM-L-6-v2 ONNX), but first-query latency is non-trivial. Fine for a persistent MCP server; painful for one-shot invocations.

4. **Coverage gap vs Grep on edit-adjacent symbols.** Function-level chunking means *edits inside a function* aren't surfaced until re-ingest. Grep finds them instantly.

5. **Reranker model constraint.** ms-marco-MiniLM-L-6-v2 is required — built-in fastembed models don't cut it. Not a blocker, just a dependency note.

---

## Recommended integration shape

### Layer 1 — MCP server binary (`code-rag-mcp`)

New binary in the workspace root `[[bin]]` list. Speaks MCP over stdio. Reuses existing `AppState`-style setup (LanceStore, Embedder, Reranker, IntentClassifier). Exposes **three tools**, matching the engine's natural seams:

| Tool | Wraps | Returns |
|---|---|---|
| `code_rag_search(query, intent?)` | Full `retriever::retrieve` minus LLM | `Vec<SourceInfo>` (file/line/excerpt/score/intent) |
| `code_rag_graph(identifier, direction)` | `graph::detect_direction` + `graph_augment` directly | Callers/callees with file:line, tier-scored |
| `code_rag_overview(topic)` | Forces `QueryIntent::Overview` routing | README/crate/module_doc/folder chunks |

All three must have a **no-LLM** path — Claude Code is the LLM, code-rag is the retriever. The existing `send_chat_rag_only` ([crates/code-rag-ui/src/standalone_api.rs:41](crates/code-rag-ui/src/standalone_api.rs#L41)) is the pattern to mirror server-side.

**Excerpt vs full content:** return ~30-line excerpts with file/line; let Claude decide whether to `Read` the file. Claude Code's context is scarce; dumping full `code_content` would defeat the point.

### Layer 2 — Claude Code Skill file

A markdown Skill that tells Claude *when* to reach for the MCP tools vs. Grep/Glob/Read. Key rules:

- **Grep** for exact identifiers, error strings, known symbols, recently-edited code.
- **code_rag_search** for "how does X work", "where's the Y logic", conceptual/architectural queries.
- **code_rag_graph** for "what calls X", "what does X call", flow/dependency questions.
- **code_rag_overview** for onboarding to an unfamiliar repo.
- **Prerequisite**: Skill includes a one-line check — "if `./data/` is missing, ask user to run ingest first."
- **Staleness contract**: "results reflect last ingest; for files edited this session, prefer Grep/Read."

### Layer 3 — Ingest UX (the weakest link, flag but don't solve)

A single-repo-in-place ingest mode (`code-rag-ingest .` without the sibling-folder convention) is the main UX work. A watch mode or auto-re-ingest-on-query is a future enhancement, not scoped here.

---

## Why this differs from Claude Code's built-ins

Claude Code already has Grep (ripgrep), Glob, Read. It does not have:
- Intent classification that *routes* across chunk types (README vs code vs module_doc vs folder).
- A persistent call graph with 3-tier symbol resolution.
- Cross-encoder reranking on retrieved candidates.
- Cross-type score-comparable ranking (code competing with README against the same relevance scale).

Those are the four things that justify the integration. If you'd only ever ask "find the function named `foo`," this is overkill — Grep already wins.

---

## Open questions to resolve before building

1. **Single-repo ingest flag** — does [crates/code-raptor/src/main.rs](crates/code-raptor/src/main.rs) already support `--root .` or does it need one?
2. **MCP transport** — stdio (per-session) or streamable HTTP (shared across sessions on the same repo)? Stdio is simpler for feasibility; HTTP amortizes model load across multiple Claude Code instances.
3. **Intent hint vs auto-classify** — accept `intent: Option<QueryIntent>` on `code_rag_search` so Claude can override the classifier when it has strong priors (e.g. "I know this is an overview question")? The pre-classify hook ([intent::pre_classify_comparison](crates/code-rag-engine/src/intent.rs)) is already a precedent for hard overrides.
4. **Excerpt length budget** — 30 lines? 50? Tune against typical Claude Code context pressure.

---

## Verification plan (if this graduates to implementation)

- **E2E smoke**: run `cargo run --bin code-rag-mcp` in stdio mode, pipe a JSON-RPC `tools/list` request, confirm the three tools advertise. Then `tools/call code_rag_search` with a known query from the harness dataset, compare result set to `cargo run --bin code-rag-harness` on the same query.
- **Quality regression**: reuse the 81-query harness dataset. The MCP path should return the same `RetrievalResult` as the HTTP `/chat` path on the same index (barring LLM). If recall@5 diverges from the V3.3 baseline, the MCP wrapper has a bug, not the engine.
- **Skill wiring**: drop the Skill file in `.claude/skills/` on a test repo, run Claude Code against "what's the architecture here?" and "what calls `retrieve`?" — confirm the right tool is chosen.
- **Staleness test**: ingest, edit a function body, ask `code_rag_search` about it; confirm stale content is returned and the Skill's Grep fallback guidance kicks in.
