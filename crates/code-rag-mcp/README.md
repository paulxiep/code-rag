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

## Install

### From source (workspace clone)

```bash
git clone https://github.com/paulxiep/code-rag
cd code-rag
cargo build --release -p code-rag-mcp -p code-raptor
# Binaries land in target/release/code-rag-mcp and target/release/code-raptor
# Copy them onto PATH, e.g.:
cp target/release/code-rag-mcp target/release/code-raptor ~/.local/bin/
```

### Via cargo install (once published)

```bash
cargo install code-rag-mcp code-raptor
```

## Set up against a repository

One-time ingest + wire Claude Code:

```bash
cd /path/to/your/repo

# 1. Build the index (tens of seconds to a few minutes depending on repo size).
#    First run downloads the BGE-small embedder and the reranker ONNX from HF.
code-raptor ingest . --db-path ./.code-rag/index.lance --single-repo --full

# 2. Drop the Skill file into your repo so Claude Code routes queries correctly.
#    Skill path in this repo: crates/code-rag-mcp/skills/code-rag.md
mkdir -p .claude/skills
cp /path/to/code-rag/crates/code-rag-mcp/skills/code-rag.md .claude/skills/

# 3. Register the MCP server with Claude Code. Write .mcp.json in the repo:
cat > .mcp.json <<'JSON'
{
  "mcpServers": {
    "code-rag": {
      "command": "code-rag-mcp",
      "args": [
        "--db-path", "./.code-rag/index.lance",
        "--repo-path", "."
      ]
    }
  }
}
JSON
```

Launch Claude Code in the repo. Verify the server is connected via `/mcp` — you should see `code-rag` with 5 tools. Ask a conceptual question like "how is this codebase organised?" and the skill will route it to `code_rag_overview`.

## After editing code

The index is a snapshot. When you've made meaningful edits and want retrieval to see them:

```
Ask Claude Code: "run code_rag_reindex"
```

It invokes `code-raptor ingest . --db-path ./.code-rag/index.lance --single-repo --full` under the hood and blocks until done. For one-off lookups in a single just-edited file, prefer `Grep` / `Read` — they're faster and free.

## CLI flags

```
code-rag-mcp [OPTIONS]

  --db-path <PATH>           Path to the LanceDB index (default: ./.code-rag/index.lance)
  --repo-path <PATH>         Repo directory for code_rag_neighbors and reindex (default: .)
  --code-raptor-bin <NAME>   Binary for code_rag_reindex to spawn (default: code-raptor)
  --model <NAME>             LLM model name (unused — MCP never calls the LLM)
  --no-rerank                Disable the cross-encoder reranker
```

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
code-rag-mcp --db-path ./.code-rag/index.lance
```

Or disable the reranker entirely with `--no-rerank` (recall drops ~5 points but startup is instant).

## Supported languages

The ingestion pipeline uses tree-sitter parsers for Rust, Python, TypeScript, and JavaScript. Other languages are silently skipped. READMEs and folder-level summaries are language-agnostic.

## Troubleshooting

**"AppState init failed: ... code_chunks table not found"** — the index doesn't exist yet. Run `code-raptor ingest . --db-path ./.code-rag/index.lance --single-repo --full`.

**Reranker download hangs on first run** — large network fetch (~90 MB). Disable with `--no-rerank` or set `CODE_RAG_RERANKER_DIR`.

**`code_rag_reindex` fails: "failed to spawn code-raptor"** — the binary isn't on PATH. Either add it, or pass `--code-raptor-bin /full/path/to/code-raptor`.

**`code_rag_neighbors` error: "chunk_id not found"** — neighbors currently supports code chunks only; README/folder/module-doc chunks aren't resolvable by chunk_id yet. Read the file directly.

**Stale results after editing code** — the index is a snapshot. Call `code_rag_reindex` or prefer Grep/Read for just-edited files.

## License

Apache-2.0 (same as the parent workspace).
