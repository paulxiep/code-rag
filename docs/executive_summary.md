# Portfolio RAG Chat — Executive Summary

## What It Is

A RAG (Retrieval-Augmented Generation) chatbot that answers questions about code repositories. Parses Rust, Python, and TypeScript codebases with tree-sitter, extracts docstrings and call graphs, generates embeddings with FastEmbed, stores in LanceDB, and responds via Google Gemini. Intent classification routes queries to optimized retrieval strategies, and retrieval traces surface all sources with relevance scores.

## Why It Matters

- **Portfolio showcase**: Demonstrates Rust, RAG architecture, and chatbot development skills
- **Meta-project**: Can answer questions about itself and other portfolio projects
- **Code understanding**: Semantic search over function-level code chunks

## Key Features

- **Multi-language parsing**: Rust, Python, and TypeScript via tree-sitter AST queries
- **Docstring extraction**: `///` (Rust), `"""` (Python), `/** */` (TypeScript JSDoc) — enriches embeddings and LLM context
- **Call graph extraction**: Direct + method calls extracted from AST, enriched into embeddings
- **Intent classification**: Cosine similarity against prototype query embeddings — semantic, not keyword-based
- **Query routing**: Declarative routing table maps intent (overview, implementation, relationship, comparison) to per-type retrieval limits
- **Retrieval traces**: All 4 chunk types surfaced with relevance scores, sorted by relevance — the system shows its work
- **Quality harness**: 43-query test dataset (3-tier: hero regression anchors, directional per-intent, pipeline-agnostic smoke tests) with automated recall@K, MRR, intent accuracy, and latency measurement — dual-run mode isolates classifier vs. retrieval quality
- **Multi-binary crate**: `src/lib.rs` extraction enables `code-rag-harness` second binary alongside the main server — shared library, independent entry points
- **Centralized chunk flattening**: `FlatChunk` + `flatten()` — single source of truth for both API responses and harness evaluation
- **Incremental ingestion**: SHA256 file hashing skips unchanged files for fast re-indexing
- **4 chunk types**: Code functions, README files, Crate metadata, Module docs
- **Trait-based language abstraction**: Add new languages by implementing `LanguageHandler` trait
- **Vector search**: LanceDB with FastEmbed (BGE-small-en-v1.5, 384 dimensions)
- **LLM integration**: Google Gemini via rig-core (optional — retrieval works without API key)
- **Web UI**: Leptos WASM SPA (Rust compiled to WebAssembly)
- **GitHub Pages demo**: Full RAG pipeline runs in-browser via `standalone` feature — no backend needed
- **Shared engine**: `code-rag-engine` crate compiles to both native and wasm32

## Quick Start

```bash
# 1. Ingest repositories
docker-compose -f docker-compose-ingest.yaml up

# 2. Run query server
docker-compose up
```

Open http://localhost:3000 for the chat interface.

## Baseline Quality Metrics

Measured against 43 test queries across 4 intent categories:

| Metric | Full Pipeline | Ground-Truth Intent |
|--------|--------------|-------------------|
| recall@5 | 0.65 | 0.67 |
| MRR | 0.60 | 0.61 |
| Intent accuracy | 62% | 100% |

Per-intent recall@5: overview 1.00, implementation 0.70, comparison 0.75, relationship 0.38. The +0.02 delta between runs proves retrieval quality — not classification — is the bottleneck.

## Current State

192 tests, 0 warnings:
- `code-raptor`: Ingestion CLI — trait-based language handlers, incremental ingestion, docstring + call extraction, data export
- `code-rag-engine`: Shared algorithms — intent classification, context building, scoring (compiles to native + wasm32)
- `code-rag-store`: Embedder + VectorStore — scored search API, distance-aware retrieval
- `code-rag-types`: Shared types — UUID chunk IDs, content hashes, nullable docstrings
- `code-rag-chat`: Query API — retrieval, LLM, quality harness, serves WASM UI
- `code-rag-ui`: Leptos WASM SPA — default mode calls backend API, standalone mode runs full RAG pipeline in-browser

## Technology

- **Language**: Rust
- **Web Framework**: Axum 0.8
- **LLM**: Google Gemini (rig-core 0.27)
- **Vector Database**: LanceDB
- **Embeddings**: FastEmbed (BGE-small-en-v1.5)
- **Code Parsing**: tree-sitter (Rust, Python, TypeScript/TSX)
- **Frontend**: Leptos 0.8 (Rust WASM, CSR)
- **Deployment**: Docker (local) + GitHub Pages (static demo)
