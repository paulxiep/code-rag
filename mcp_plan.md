# code-rag MCP server + Claude Code Skill — implementation plan

## Context

Tracks A1–A4, B1–B5, and C1–C3 are complete. A5 (repo summaries) remains planned but is not a prerequisite for MCP work — `code_rag_overview` already routes README/crate/folder/module_doc chunks via A2+A3. The retrieval brain (4-intent routing, hybrid+rerank, call graph, comparison decomposition) is mature. Goal is portfolio + learning: test best-practice insights from other MCP code-RAG servers against our own retriever, and ship a deployable app you can point at any single repo, ingest it, and use as a Claude Code Skill.

Original feasibility assessment lives at [code-rag-mcp.md](code-rag-mcp.md). Pipeline seams referenced there are all still present and correctly shaped. This plan replaces the "is it feasible" question with "build it in these phases."

Post-A4 harness numbers that motivate this: relationship 0.38 → 0.59 (+0.21), implementation 0.70 → 0.74, aggregate recall@5 = 0.72 on the 81-query dataset. Relationship is the intent grep structurally cannot serve — the one a call-graph MCP tool best exploits.

## External research trail (best practices to absorb)

| Project | Lesson |
|---|---|
| [zilliztech/claude-context](https://github.com/zilliztech/claude-context) | ~40% token reduction vs grep-only — target bar. 3-tool API (`index_codebase`, `search_code`, `clear_index`). Merkle-tree incremental reindex. |
| [shinpr/mcp-local-rag](https://github.com/shinpr/mcp-local-rag) | `read_chunk_neighbors` tool — expand context around a hit without re-querying. Steal this. `status` and `list_files` are cheap debuggability wins. |
| [doITmagic/rag-code-mcp](https://github.com/doITmagic/rag-code-mcp) | Local-first (Ollama + Qdrant). We already match this. |
| [Code Graph RAG MCP](https://mcpservers.org/servers/github-com-er77-code-graph-rag-mcp) | Validates graph-augmented retrieval as a defensible angle (our C1–C2 investment). |
| [Milvus blog: against grep-only retrieval](https://milvus.io/blog/why-im-against-claude-codes-grep-only-retrieval-it-just-burns-too-many-tokens.md) | Position piece — useful framing for portfolio prose. |
| [Skills vs MCP Servers 2026](https://dev.to/williamwangai/claude-code-skills-vs-mcp-servers-what-to-use-how-to-install-and-the-best-ones-in-2026-548k) | Skill = *when to use*, MCP = *capability*. Both load-bearing. |

**Concrete insights to absorb:**
- Tool count: 4, not 3. Our original writeup proposed `code_rag_search`, `code_rag_graph`, `code_rag_overview`. Add `code_rag_neighbors(chunk_id, window)` inspired by shinpr.
- Publish a token-reduction number. Claude Context published 40%. Our harness can compute the same metric — defensible portfolio claim.
- Incremental reindex on ingest call. Make `code_rag_reindex` an MCP tool Claude can call itself when it senses staleness.
- Skill routes by intent category, not "use this for everything." Otherwise we're indistinguishable from generic hybrid-search MCPs.

---

## Phase 0 — Single-repo ingest (prerequisite)

**Size:** small. 1–2 days.

The original writeup's "weakest link." Ingestion still assumes a parent folder with sibling projects ([README.md:10-20](README.md#L10-L20)). Must work against `.` before any MCP tool ships.

**Changes:**
- [crates/code-raptor/src/main.rs](crates/code-raptor/src/main.rs) — add `--single-repo` flag (or auto-detect when `--project-name` is set and no sibling folders exist). When set, chunk paths are repo-relative forward-slash without a portfolio-project prefix.
- Verify [crates/code-rag-store](crates/code-rag-store/) and [crates/code-rag-ui](crates/code-rag-ui/) WASM export work on single-repo indexes (path strings are opaque to retrieval — should be transparent).
- Run the existing harness against a single-repo index of this repo to confirm no regression vs. the portfolio ingest.
- Update [README.md](README.md) with the single-repo flow as the MCP deployment path.

**Done when:** `cargo run -p code-raptor -- ingest . --single-repo --db-path ./data/single.lance` ingests this repo standalone, and `cargo run --bin code-rag-harness -- --db-path ./data/single.lance` runs without error and returns sensible per-intent recall.

**Baseline protection (do not skip):**
- The harness run on `single.lance` is a smoke test of the single-repo ingest path only. Cross-project queries in `data/test_queries.json` whose `expected_files` reference sibling projects (e.g. `invoice-parse`) are **expected to fail** because those chunks are absent — this is not a regression.
- Track-comparison runs continue to use `data/portfolio.lance` against the parent dir per `project_harness_baseline.md`. **Never overwrite `data/portfolio.lance` from a single-repo ingest.**
- **Phase 0.5 verification:** After the `--single-repo` flag lands, re-run the existing parent-dir ingest into `portfolio.lance` and confirm aggregate recall@5 matches the V3.3 + post-A4 baseline. The multi-project path is the existing baseline contract; the new flag must not regress it.

---

## Phase 1 — MCP spike: `code_rag_search` only

**Size:** small. 1–2 days.

Prove the wire-up works end-to-end before investing in the full tool surface.

**Prerequisite (decide before starting):**
- **MCP Rust SDK choice — pick now, not at phase start.** Evaluate `rmcp` (the official Rust SDK) maturity. Commit to either "use it" or "hand-roll JSON-RPC 2.0 over stdio." This is a binary dependency call that determines whether the spike is half a day or two days, so resolve at plan time.

**Build:**
- New binary: `crates/code-rag-mcp/src/main.rs` (or `src/bin/code-rag-mcp.rs`), add `[[bin]]` entry in [Cargo.toml](Cargo.toml).
- Stdio MCP server using the SDK chosen in the prerequisite above.
- Reuses the same `AppState` setup as [src/api/handlers.rs:42](src/api/handlers.rs#L42): LanceStore, Embedder, Reranker, IntentClassifier, CallGraph.
- **Process model:** decide single-request-at-a-time vs concurrent. LanceDB and the reranker ONNX session both need an explicit choice (mutex vs pool). One sentence in the binary's comments is enough; surfacing the decision is what matters.
- Exposes **one** tool for now:
  - `code_rag_search(query: string, intent?: "overview"|"implementation"|"relationship"|"comparison")` — wraps [retriever::retrieve](src/engine/retriever.rs) minus LLM. Mirrors the no-LLM pattern at [standalone_api.rs:41](crates/code-rag-ui/src/standalone_api.rs#L41) (`send_chat_rag_only`).
  - Returns `Vec<SourceInfo>` from [src/api/dto.rs:22](src/api/dto.rs#L22), serialized as MCP tool result content with 30-line excerpts + `chunk_id` + `path:line` + `relevance`.
- **`chunk_id` plumbing (promoted from Phase 2 — must land in this phase):**
  - `chunk_id: String` already exists on every chunk type in [crates/code-rag-types/src/lib.rs](crates/code-rag-types/src/lib.rs) (deterministic SHA-based via `deterministic_chunk_id`), but is **dropped during flatten**.
  - Add `chunk_id: String` to `FlatChunk` at [crates/code-rag-engine/src/retriever.rs:187-194](crates/code-rag-engine/src/retriever.rs#L187-L194) and populate during `flatten()`.
  - Mirror onto `SourceInfo` at [src/api/dto.rs:22](src/api/dto.rs#L22).
  - Verify the identical `chunk_id` appears in WASM `send_chat_rag_only` output for cross-surface consistency.
  - Why here, not Phase 2: this change cuts across the engine crate, the server DTO, and the WASM standalone API. Discovering it mid-Phase-2 (where `code_rag_neighbors` first needs it) would stall the full-tool-surface work. Land it with the spike.

**Done when:**
- `cargo run --bin code-rag-mcp` in stdio mode, piped a JSON-RPC `tools/list` request, advertises `code_rag_search`.
- `tools/call code_rag_search` with a known harness query returns the same `RetrievalResult` ordering as `cargo run --bin code-rag-harness` on the same query.
- Registered as an MCP server in a test Claude Code config and answered at least one query through it.

---

## Phase 2 — Full tool surface (4 tools)

**Size:** medium. 3–4 days.

Add the remaining three tools on top of Phase 1's skeleton.

| Tool | Wraps | Returns |
|---|---|---|
| `code_rag_graph(identifier, direction?: "callers"\|"callees"\|"both")` | [graph::detect_direction](crates/code-rag-engine/src/graph.rs) + `graph_augment` directly (bypasses vector search for pure graph queries) | Callers/callees with file:line, tier-scored (1/2/3 symbol resolution tiers) |
| `code_rag_overview(topic?)` | Forces `QueryIntent::Overview` routing via [intent::route](crates/code-rag-engine/src/intent.rs) | README + crate + folder + module_doc chunks, intent-weighted |
| `code_rag_neighbors(chunk_id, window?: int = 20)` | Direct fetch by `chunk_id` + line-window file read | File excerpt of the surrounding window — lets Claude expand a hit without a full `Read` |

All tools remain no-LLM. `code_rag_neighbors` is the novel one — new helper in [crates/code-rag-store](crates/code-rag-store/) to look up a chunk by id and return its file path + start/end lines, then read the expanded window. (Relies on the `chunk_id` plumbing landed in Phase 1.)

**Optional 5th tool (decide after Phase 2 usability feedback):**
- `code_rag_reindex(paths?: [string])` — triggers incremental ingest against the live index. Matches Claude Context's Merkle-tree story. Worth it if staleness bites in real use; skip if the Skill's "prefer Grep for just-edited code" rule covers it.

**Done when:**
- All 4 tools advertise via `tools/list` and return correct shapes via `tools/call`.
- Hero query tests pass through MCP: "what calls `retrieve`?" returns graph results; "what does engine/ do?" returns folder + README chunks; "expand neighbors of `<chunk_id>`" returns a file window.
- Unit tests added per tool in the `code-rag-mcp` crate.

---

## Phase 3 — Claude Code Skill file

**Size:** small. 1 day.

The Skill is what makes the intent-routing investment pay off. Without it, Claude reaches for Grep on 80% of queries by default.

**Build:** `.claude/skills/code-rag.md` shipped in this repo as the deployment exemplar. Rules:

- **Grep/Read first** for exact identifiers, error strings, symbols edited this session.
- **`code_rag_search`** for "how does X work", "where is the Y logic" (Implementation intent).
- **`code_rag_graph`** for "what calls X", "what does X call", flow/dependency (Relationship intent).
- **`code_rag_overview`** for onboarding/architecture (Overview intent).
- **`code_rag_neighbors`** after any `code_rag_search` hit, before `Read`, to expand cheaply.
- **Prerequisite check:** one line — "if `./data/` is missing, run `cargo run -p code-raptor -- ingest . --single-repo`."
- **Staleness contract:** "results reflect last ingest; for files edited this session, prefer Grep/Read."

**Done when:**
- Skill file installed in `.claude/skills/` on a test repo (not this one), run Claude Code against representative queries of each intent, confirm the right tool is chosen.
- Skill file also committed to this repo under `skills/` or the equivalent so users can drop it in when deploying.

---

## Phase 4 — Deployment packaging for external repos

**Size:** small. 1 day.

The whole point is "point it at another repo, ingest, use." Must be trivial.

**Build:**
- A release script / `justfile` target / short README section that, given a target repo directory, runs:
  1. `cargo run -p code-raptor -- ingest <target> --single-repo --db-path <target>/.code-rag/index.lance`
  2. Writes a `<target>/.claude/skills/code-rag.md` pointing at that index
  3. Writes a `<target>/.mcp.json` (or user-scope equivalent) wiring the `code-rag-mcp` binary with the right `--db-path`
- Decide: does the binary load reranker ONNX models from a fixed path (bundled) or an env var? Default to bundled for zero-config.
- Document required env: `HF_HUB_OFFLINE` handling, model cache directory, reranker model file.

**Done when:** a fresh clone of a non-portfolio Rust/Python/TS repo goes from `git clone` to "ask Claude Code 'what calls X?'" in under 5 minutes using documented steps.

---

## Phase 5 — Portfolio measurement writeup

**Size:** medium. 2–3 days.

The differentiator from a generic MCP RAG server. Produces the receipts.

**Build:** `docs/mcp-benchmark.md` reporting head-to-head vs. [zilliztech/claude-context](https://github.com/zilliztech/claude-context) on this repo.

**Methodology:**
- Index this repo with both `code-rag-mcp` and Claude Context (follow their setup — it's the fastest to stand up; Milvus/Zilliz free tier or their local mode).
- Run the 81-query harness through both (direct MCP `tools/call`, not via Claude Code — deterministic measurement).
- Report per-intent recall@5 and **token cost per query** (sum of returned excerpt tokens). Token cost is the selling number — mirrors Claude Context's own 40% claim.
- Write up what we learned from their design (what we kept, what we changed) and what our intent routing + graph tools add on top.

**Done when:** benchmark table published, portfolio-ready, linked from [README.md](README.md) and [development_plan.md](development_plan.md) success metrics section.

---

## What to expect when deployed

- **Setup friction** is the UX risk. First run costs an ingest + reranker ONNX load. Amortized after that via the long-lived MCP server.
- **Relationship and overview queries will feel qualitatively better than Grep.** Implementation queries with exact identifiers will feel same-ish to Grep (B2 hybrid search narrows the gap but doesn't beat ripgrep on known strings). This is the intent-routing argument made tangible.
- **Staleness will bite during active editing.** Skill's "prefer Grep for just-edited code" line is doing real work. `code_rag_reindex` (optional Phase 2 tool) only partially mitigates.
- **Token savings should land near competitor claims (~30–40%).** Measurable via Phase 5's harness.

## Risks & open decisions

| Risk / decision | Resolution |
|---|---|
| MCP Rust SDK maturity | **Resolved at plan time, not Phase 1 start.** Evaluate `rmcp` now; commit to "use it" or "hand-roll JSON-RPC over stdio" before opening the spike PR. (Moved to Phase 1 prerequisite.) |
| Reranker model distribution (ms-marco-MiniLM-L-6-v2 per memory `feedback_reranker_model.md`) | Bundle ONNX weights in the release, or download-on-first-run with a cache. Default to bundle for zero-config. Couples to Phase 4's distribution choice — pick `cargo install` / GitHub Releases binary / `docker run` together. |
| `chunk_id` plumbing (was: stability) | **Resolved scope-wise: lands in Phase 1**, not Phase 2. The id is already SHA-stable on chunk types but is dropped during flatten — see Phase 1 build list. |
| Should `code_rag_reindex` ship in Phase 2? | Default: skip. Add in Phase 2.5 only if real-use feedback shows staleness is painful. Note: this is the one tool that directly addresses the staleness story in [code-rag-mcp.md](code-rag-mcp.md), so if portfolio narrative leans on staleness handling, reconsider. |
| Single-repo non-Rust targets (Python-only, TS-only repos) | Phase 0's single-repo flag must be language-agnostic. Verify against a non-Rust test repo in Phase 4. |
| Phase 5 benchmark fairness | The 81-query harness was authored against this codebase's structure — Claude Context will look weaker than it would on a neutral repo. Either acknowledge in the writeup or run a secondary benchmark on a third-party repo. |

## Critical files

- [Cargo.toml](Cargo.toml) — add `[[bin]]` for `code-rag-mcp`
- **new:** `crates/code-rag-mcp/src/main.rs` — stdio MCP server
- [crates/code-raptor/src/main.rs](crates/code-raptor/src/main.rs) — `--single-repo` flag
- [src/engine/retriever.rs](src/engine/retriever.rs) — wrap point (no changes)
- [crates/code-rag-engine/src/graph.rs](crates/code-rag-engine/src/graph.rs) — `detect_direction`/`graph_augment` for `code_rag_graph` (no changes)
- [crates/code-rag-engine/src/intent.rs](crates/code-rag-engine/src/intent.rs) — `classify`/`route` for auto intent (no changes)
- [src/api/dto.rs:22](src/api/dto.rs#L22) — `SourceInfo` reused; may add `chunk_id` if missing
- [crates/code-rag-ui/src/standalone_api.rs:41](crates/code-rag-ui/src/standalone_api.rs#L41) — pattern reference for no-LLM path
- **new:** `skills/code-rag.md` (or `.claude/skills/code-rag.md`) — shipped Skill
- [README.md](README.md) — single-repo + MCP deployment docs
- **new:** `docs/mcp-benchmark.md` — Phase 5 portfolio artifact
