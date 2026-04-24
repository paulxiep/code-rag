# code-rag-mcp

An MCP server that exposes local semantic + graph retrieval over a single repository to Claude Code (and any other MCP client). Paired with a Claude Code Skill that routes the right questions to the right tool — Grep for exact strings, `code_rag_*` for "how does X work", "what calls X", and architecture/onboarding queries.

Built on the [code-rag](../..) pipeline: intent classification, hybrid BM25 + BGE-small semantic search, cross-encoder reranking (ms-marco-MiniLM-L-6-v2), and a persisted call graph with 3-tier symbol resolution.

## What you get

Five MCP tools, all prefixed `code_rag_`:

| Tool | Purpose |
|---|---|
| `code_rag_search(query, intent?)` | Intent-routed semantic retrieval |
| `code_rag_graph(identifier, direction?)` | Callers / callees of a function |
| `code_rag_overview(topic?)` | Forces Overview intent for architecture questions |
| `code_rag_neighbors(chunk_id, window?)` | Expand a prior hit's source window |
| `code_rag_reindex()` | Full re-ingest of the repo |

Plus a bundled Claude Code [skill](skills/code-rag.md) that tells Claude when to reach for each.

## Prerequisites

- Rust toolchain (edition 2024) — needed to build from source; skip if you download a release binary.
- Claude Code CLI — https://claude.com/claude-code

That's it. The pipeline runs fully local: no API keys, no cloud dependencies.

## Install — three steps, no terminal commands once the exe is on disk

1. **Download** the zip for your platform from the [GitHub Release page](https://github.com/paulxiep/code-rag/releases). Extract somewhere persistent (e.g. `C:\tools\code-rag-mcp\` or `~/.local/share/code-rag-mcp/`). The archive contains a single `code-rag-mcp` binary plus a `code-rag-mcp.config.yaml` template.
2. **Edit** `code-rag-mcp.config.yaml` (lives next to the exe) in any text editor:

   ```yaml
   target_path: C:/Users/me/projects/my-repo   # or "." if you're running the exe from inside the repo
   workspace: false                            # set true if target_path is a parent of many sub-projects
   ```

3. **Run** the exe (double-click works). It writes `.claude/skills/code-rag.md`, `.mcp.json`, and a `.gitignore` entry into your target dir, then exits.

That's installation. To use it: open Claude Code in the target dir. The first conceptual question triggers the agent to call `code_rag_reindex mode=full` for the initial ingest automatically — no terminal commands needed.

> **Note on PATH.** Claude Code spawns `code-rag-mcp` via the generated `.mcp.json`, which uses the bare command name. Either put `code-rag-mcp` on your PATH (drag it into `~/.local/bin/`, or add the install dir to your Windows PATH), or hand-edit the generated `.mcp.json` to use the absolute path.

### From source (alternative, if you prefer building)

```bash
git clone https://github.com/paulxiep/code-rag
cd code-rag
cargo build --release -p code-rag-mcp
# Binary lands in target/release/code-rag-mcp; place it where you want it.
```

`cargo install code-rag-mcp` will work once the crate is published.

## After editing code

The index is a snapshot. When you've made meaningful edits and want retrieval to see them:

```
Ask Claude Code: "run code_rag_reindex"
```

By default this runs an **incremental** ingest — only files whose `content_hash` changed are re-embedded (typically single-digit seconds for a small edit). To wipe and rebuild the project's chunks from scratch (tens of seconds; recovery path when the index looks corrupted), pass `mode: "full"`. For one-off lookups in a single just-edited file, prefer `Grep` / `Read` — they're faster and free.

## CLI flags

The exe has three modes — bare run for setup, the Claude-Code-spawn case for serve, and the internal `ingest` subcommand `code_rag_reindex` calls. The flags below are for the serve case.

```
code-rag-mcp [OPTIONS]

  --db-path <PATH>     Path to the LanceDB index (default: ./.code-rag-mcp/index.lance)
  --repo-path <PATH>   Repo directory for code_rag_neighbors and reindex (default: .)
  --workspace          Multi-project mode: code_rag_reindex omits --single-repo so each
                       sibling subdirectory becomes its own project. Default is single-repo.
  --model <NAME>       LLM model name (unused — MCP never calls the LLM)
  --no-rerank          Disable the cross-encoder reranker
```

The setup-mode YAML config has a `workspace: bool` field that flows into the generated `.mcp.json` as `--workspace`, so you don't normally hand-set this flag.

## Offline / bundled reranker model

The reranker auto-downloads `cross-encoder/ms-marco-MiniLM-L-6-v2` from HuggingFace on first run and caches it (default HF cache: `~/.cache/huggingface/hub/`). To avoid network access entirely, pre-download the model files and point the MCP at them:

```bash
# Files you need in $MODEL_DIR:
#   model.onnx          (or onnx/model.onnx)
#   tokenizer.json
#   config.json
#   special_tokens_map.json
#   tokenizer_config.json  (optional)

export CODE_RAG_RERANKER_DIR=/path/to/ms-marco-MiniLM-L-6-v2
```

Or disable the reranker entirely with `--no-rerank` (recall drops ~5 points but startup is instant).

## Supported languages

The ingestion pipeline uses tree-sitter parsers for Rust, Python, TypeScript, and JavaScript. Other languages are silently skipped. READMEs and folder-level summaries are language-agnostic.

## Troubleshooting

**"AppState init failed: ... code_chunks table not found"** — the index doesn't exist yet. Run `code-raptor ingest . --db-path ./.code-rag/index.lance --single-repo --full`.

**Reranker download hangs on first run** — large network fetch (~90 MB). Disable with `--no-rerank` or set `CODE_RAG_RERANKER_DIR`.

**`code_rag_reindex` fails: "failed to spawn ..."** — the running MCP server can't find its own binary path. This usually only happens in unusual sandboxes (e.g. a container that's deleted the on-disk exe). Restart Claude Code so it spawns a fresh server.

**`code_rag_neighbors` error: "chunk_id not found"** — neighbors currently supports code chunks only; README/folder/module-doc chunks aren't resolvable by chunk_id yet. Read the file directly.

**Stale results after editing code** — the index is a snapshot. Call `code_rag_reindex` or prefer Grep/Read for just-edited files.

## License

Apache-2.0 (same as the parent workspace).
