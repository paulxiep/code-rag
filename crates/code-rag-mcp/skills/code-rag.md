---
name: code-rag
description: Route code-navigation queries for this repository to the right tool — Grep/Read for exact identifiers and just-edited code, code-rag MCP tools for conceptual "how does X work", call-graph "what calls X", and architecture/onboarding questions. Activate when the user asks about this repository's code, structure, or relationships.
---

# code-rag retrieval skill

This repository is indexed by [code-rag](https://github.com/paulxiep/code-rag) — a local RAG pipeline with intent classification, hybrid BM25 + semantic search, cross-encoder reranking, and a persisted call graph. It exposes five MCP tools (all prefixed `code_rag_`). Use them to answer questions about this codebase faster than Grep-only allows — but reach for Grep first whenever an exact string match will do.

## Prerequisite

Before using any `code_rag_*` tool, check whether the index exists:

- Default index path: `./.code-rag/index.lance`
- **First-time setup** — if the index is missing or empty (a `code_rag_search` returns `hits: []`), call `code_rag_reindex` with `mode: "full"` to perform the initial ingest. The MCP server starts cleanly against an empty index, so this works on a fresh repo where only the `.mcp.json` and skill have been deposited. Tens of seconds on a typical repo. No need to tell the user to run anything in their terminal.
- **Live edits** — when the user has changed files this session and the index is stale, call `code_rag_reindex` with default args (incremental mode, single-digit seconds for a small edit). For one-off lookups in a single just-edited file, `Grep` / `Read` are still faster.

## When to use which tool

Pick based on the *shape* of the question, not the topic.

### Use Grep / Read first
The indexed tools are for *conceptual* lookups. Keep using the built-in tools when:
- You know the exact identifier, error string, or import path → `Grep`
- The user just edited a file and asks about it → the index is stale; use `Read` / `Grep`
- You want to see a complete file → `Read`
- The query is one or two tokens ("find `foo`", "where's `ErrorKind::NotFound`") → `Grep` wins on speed and freshness

### `code_rag_search(query, intent?)` — conceptual retrieval
*"How does X work?" / "Where's the Y logic?" / "What handles Z?"*

Returns ranked code, README, folder-summary, and module-doc chunks. The classifier picks an intent; pass `intent` to override when you have a strong prior:
- `"implementation"` — function-level code details
- `"overview"` — README / architecture / crate-level
- `"relationship"` — prefer the `code_rag_graph` tool instead for crisp call-graph answers
- `"comparison"` — "X vs Y" or "difference between A and B"

Each hit includes a `chunk_id` — you can pass it to `code_rag_neighbors` to expand the surrounding source without reading the whole file.

### `code_rag_graph(identifier, direction?)` — call-graph traversal
*"What calls `foo`?" / "What does `bar` call?" / "Who uses `Baz`?"*

Returns direct callers/callees with file + identifier + resolution tier (1 = same-file, 2 = import-based, 3 = unique-global name match). `direction` is `"callers"`, `"callees"`, or `"both"` (default).

This is *structurally* better than Grep for call relationships — Grep gives you textual occurrences (including docstrings and unrelated tokens), while the graph is resolved call edges only. Use it for "show me the call sites" questions, even when you already know the function's file.

### `code_rag_overview(topic?)` — architecture / onboarding
*"What does this project do?" / "How is the codebase organized?" / "What are the main components?"*

Forces Overview intent — READMEs, folder summaries, module docs, and crate descriptions surface ahead of function-level code. Pass `topic` to focus ("retrieval pipeline", "storage layer"); omit for a general overview.

### `code_rag_neighbors(chunk_id, window?)` — expand a hit
*Follow-up to a `code_rag_search` or `code_rag_graph` result when the default excerpt isn't enough.*

Given a `chunk_id` from a previous hit, returns a `window`-line excerpt (default 20) around the chunk's start line. Cheaper than `Read` on the whole file and preserves the chunk's line numbering. Use it before escalating to `Read`.

### `code_rag_reindex(mode?)` — refresh the index after edits
*Call this after a batch of edits if the user is asking questions that depend on the new code.*

Defaults to `mode: "incremental"` — only files whose content changed are re-embedded (typically single-digit seconds). Pass `mode: "full"` to wipe and rebuild the project's chunks (tens of seconds; use this when the index looks corrupted or the chunk-derivation pipeline has changed and you've upgraded the binary).

Don't reindex after every small edit — for one-off lookups in a just-edited file, `Grep` / `Read` are still faster.

## Decision cheat-sheet

| User's question shape | Tool |
|---|---|
| `grep for "ENOENT"` | Grep |
| `find the fn named exactly foo_bar` | Grep |
| `I just edited X, what did it look like before?` | Read + git (index is stale) |
| `how does the auth layer work?` | `code_rag_search` |
| `where's the config loading logic?` | `code_rag_search` |
| `what calls `retrieve`?` | `code_rag_graph direction=callers` |
| `what does `handle_request` call?` | `code_rag_graph direction=callees` |
| `give me a project tour` | `code_rag_overview` |
| `high-level architecture?` | `code_rag_overview` |
| `show me more of this result` | `code_rag_neighbors chunk_id=<from prior hit>` |
| `I edited a bunch of files, re-check` | `code_rag_reindex` (incremental default) |
| `first time using this on a new repo` | `code_rag_reindex mode=full` |

## Staleness contract

Index results reflect the **last ingest**. The index does not auto-refresh. For files edited in this session, either:
- Call `code_rag_reindex` (incremental by default) before re-querying — single-digit seconds for a small edit, and now safe to call freely after each batch of edits.
- Or prefer `Grep` / `Read` for the specific just-edited files when you only need one or two facts.

If a search comes back empty when you'd expect hits (`hits: []` on a query you know should match), the index is probably empty — call `code_rag_reindex` with `mode: "full"` to do the initial ingest.
