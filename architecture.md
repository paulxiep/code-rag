# CodeRAG: Architecture & Technical Design

A formal architecture document for the CodeRAG system, enforcing **Declarative**, **Modular**, and **Separation of Concerns** principles.

---

## 1. Overview

### 1.1 Document Purpose

This document serves as the authoritative technical reference for the CodeRAG system architecture. It is intended for:

- **Contributors**: Understanding system structure before implementing features
- **Reviewers**: Evaluating architectural decisions and code quality
- **Portfolio Reviewers**: Assessing technical design capabilities

This is a living document that evolves with the codebase. For feature roadmap, see [development_plan.md](../development_plan.md). For vision and differentiation, see [project-vision.md](../project-vision.md).

### 1.2 System Summary

**CodeRAG** is a Rust code understanding system that answers questions about code repositories using Retrieval-Augmented Generation (RAG).

**Value Proposition**: *Decouple knowledge from reasoning.*

Rich, structured retrieval amplifies *any* model—cheap or frontier. By offloading "what context is relevant" to the retrieval layer, the model's complexity budget can be spent on reasoning, multi-step workflows, or tool orchestration. This scales independently of model choice: better retrieval benefits Haiku and Opus alike.

### 1.3 Scope

**What CodeRAG does:**
- Semantic search over function-level code chunks
- Answer questions about unfamiliar/third-party codebases
- Multi-repo ingestion and querying
- Architecture-level understanding (folder/module/function hierarchy)

**Anti-goals (not competing here):**
- Real-time code completion (Copilot is better)
- Code generation from scratch
- IDE integration or inline quick fixes

---

## 2. Architectural Principles

### 2.1 Core Tenets

| Principle | Meaning | Enforcement |
|-----------|---------|-------------|
| **Declarative** | Describe *what*, not *how*. Config over code. Data-driven behavior. | Configuration schemas for retrieval limits, intent routing, ignored directories. Rules are data, not if-else chains. |
| **Modular** | Components are self-contained, swappable, and independently testable. | Trait-based interfaces. No cross-crate coupling except via shared types. Each crate has a single update frequency. |
| **SoC** (Separation of Concerns) | Each module has ONE job. No god objects. Clear boundaries. | code-raptor = indexing. code-rag-chat = querying. code-rag-engine = algorithms. code-rag-ui = frontend. code-rag-types = type definitions only (no logic). code-rag-store = embedding + persistence. |

### 2.2 Before Writing Code, Ask:

1. Am I describing behavior or implementing mechanics? (Declarative)
2. Can this be swapped out without ripple effects? (Modular)
3. Does this component have exactly one responsibility? (SoC)

### 2.3 Design Constraints

- **LanceDB as sole coupling point**: Producer (code-raptor) and consumer (code-rag-chat) communicate via LanceDB schema only, not code imports
- **No shared runtime state**: Each crate can run independently; no in-memory coupling
- **Types-only shared crate**: `code-rag-types` contains only struct definitions with serde, no business logic
- **Pure algorithm crate**: `code-rag-engine` contains no I/O — compiles to both native and wasm32

---

## 3. C4 Model Diagrams

### 3.1 Level 1: Context Diagram

```
                    ┌─────────────────┐
                    │    Developer    │
                    │  (queries code) │
                    └────────┬────────┘
                             │
              ┌──────────────┴──────────────┐
              ▼                             ▼
    ┌──────────────────┐          ┌──────────────────┐
    │  Docker Mode     │          │  GitHub Pages    │
    │  POST /api/chat  │          │  (standalone)    │
    └────────┬─────────┘          └────────┬─────────┘
             ▼                             ▼
┌──────────────────────────┐   ┌──────────────────────────┐
│   Axum Server            │   │   Leptos WASM SPA        │
│   code-rag-chat          │   │   (in-browser RAG)       │
│   + code-rag-engine      │   │   + code-rag-engine      │
└────────────┬─────────────┘   └────────────┬─────────────┘
             │                              │
             ▼                              ▼
┌──────────────────────────┐   ┌──────────────────────────┐
│      Gemini LLM          │   │  Gemini LLM (optional)   │
│      (Google)            │   │  (requires OAuth/API key) │
└──────────────────────────┘   └──────────────────────────┘

┌──────────────┐
│     Code     │───▶ code-raptor (ingestion) ───▶ LanceDB
│ Repositories │
└──────────────┘
```

**Two Deployment Modes:**
- **Docker**: Axum serves the Leptos WASM frontend + JSON API. Server-side embedding, LanceDB queries, and LLM generation.
- **GitHub Pages**: Fully static. The `standalone` feature flag runs the entire RAG pipeline in-browser via WASM (transformers.js embeddings, brute-force vector search, code-rag-engine algorithms). LLM generation is optional, unlocked via Google OAuth2 or API key.

**External Actors:**
- Developer: Submits natural language queries about code
- CI/CD: Triggers ingestion, export, and GitHub Pages deployment

**External Systems:**
- Gemini LLM: Google's language model for response generation (required in Docker, optional in GitHub Pages)
- Filesystem: Source code repositories to be indexed

### 3.2 Level 2: Container Diagram

```
┌─────────────────────────────────────────────────────────────────────┐
│                         Cargo Workspace                              │
├─────────────────────────────────────────────────────────────────────┤
│                                                                      │
│  ┌─────────────────────┐       ┌─────────────────────┐              │
│  │     code-raptor     │       │   code-rag-chat     │              │
│  │    (CLI Binary)     │       │   (Web Server)      │              │
│  │                     │       │   - root crate      │              │
│  │  - tree-sitter      │       │   - Axum 0.8        │              │
│  │  - walkdir          │       │   - rig-core (LLM)  │              │
│  │  - clap             │       │   - serves UI       │              │
│  └──────────┬──────────┘       └──────────┬──────────┘              │
│             │ writes                       │ reads                   │
│             ▼                              ▼                         │
│  ┌───────────────────────────────────────────────────┐              │
│  │                  code-rag-store                    │              │
│  │  - Embedder (FastEmbed BGE-small-en-v1.5)         │              │
│  │  - VectorStore (LanceDB)                          │              │
│  └───────────────────────┬───────────────────────────┘              │
│                          │                                           │
│                          ▼                                           │
│  ┌───────────────────────────────────────────────────┐              │
│  │                  code-rag-types                    │              │
│  │  - CodeChunk, ReadmeChunk                         │              │
│  │  - CrateChunk, ModuleDocChunk                     │              │
│  │  - (serde only, no logic)                         │              │
│  └───────────────────────────────────────────────────┘              │
│                                                                      │
│  ┌───────────────────────────────────────────────────┐              │
│  │                 code-rag-engine                    │              │
│  │  - IntentClassifier (cosine similarity)           │              │
│  │  - Context building, routing, scoring             │              │
│  │  - Pure Rust, no I/O — compiles to wasm32         │              │
│  └───────────────────────────────────────────────────┘              │
│       ▲ used by code-rag-chat (native)                               │
│       ▲ used by code-rag-ui (WASM, standalone mode)                  │
│                                                                      │
│  ┌───────────────────────────────────────────────────┐              │
│  │                  code-rag-ui                       │              │
│  │  - Leptos 0.8 WASM SPA (CSR)                     │              │
│  │  [default] API client → code-rag-chat             │              │
│  │  [standalone] In-browser RAG pipeline             │              │
│  └───────────────────────────────────────────────────┘              │
│                                                                      │
└─────────────────────────────────────────────────────────────────────┘
                          │
                          ▼
              ┌───────────────────────┐
              │   LanceDB Database    │
              │   (4 vector tables)   │
              │                       │
              │  - code_chunks        │
              │  - readme_chunks      │
              │  - crate_chunks       │
              │  - module_doc_chunks  │
              └───────────────────────┘
```

**Crate Responsibilities:**

| Crate | Responsibility | Update Frequency |
|-------|----------------|------------------|
| `code-raptor` | Parse code, extract chunks, embed, store, export | On code changes |
| `code-rag-chat` | Query API (Axum), retrieval, LLM generation, serves UI, quality harness (second binary) | On user queries / quality measurement |
| `code-rag-engine` | Pure algorithms: intent classification, context building, scoring | On algorithm changes |
| `code-rag-ui` | Leptos WASM frontend (default: API client, standalone: in-browser RAG) | On UI changes |
| `code-rag-store` | Embedding model, vector database ops | Shared infrastructure |
| `code-rag-types` | Type definitions (data contracts) | On schema changes |

### 3.3 Level 3: Component Diagram

#### code-raptor Components

```
┌─────────────────────────────────────────────────────────────┐
│                        code-raptor                           │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  ┌──────────────┐                                           │
│  │   main.rs    │  CLI entry point + orchestration           │
│  │              │  Subcommands:                              │
│  │              │  - ingest <path> [--full] [--dry-run]      │
│  │              │  - status                                  │
│  │              │  - export --db-path --output               │
│  │              │  Batch embedding (size: 25)                │
│  └──────┬───────┘                                           │
│         │                                                    │
│         ├──────────────────────────────────────────┐        │
│         ▼                                          ▼        │
│  ┌──────────────────────────────────────┐  ┌────────────┐  │
│  │         ingestion/mod.rs             │  │ export.rs  │  │
│  │  run_ingestion() - parse pipeline    │  │ run_export │  │
│  │  - walkdir + file categorization     │  │ Reads all  │  │
│  │  - chunk extraction (4 chunk types)  │  │ 4 tables + │  │
│  │  -> (IngestionResult, CallMap)       │  │ embeddings │  │
│  └──────┬───────────────────────────────┘  │ -> JSON    │  │
│         │                                  └────────────┘  │
│         ▼                                                    │
│  ┌──────────────────────────────────────────────────┐       │
│  │  ingestion/language.rs + languages/*.rs           │       │
│  │  LanguageHandler trait:                           │       │
│  │    name(), extensions(), grammar(),               │       │
│  │    query_string(), extract_docstring(),           │       │
│  │    extract_calls()  (V2.1)                        │       │
│  │  Implementations: RustHandler, PythonHandler,     │       │
│  │    TypeScriptHandler                              │       │
│  │  handler_for_path() registry                      │       │
│  └──────┬───────────────────────────────────────────┘       │
│         │                                                    │
│         ▼                                                    │
│  ┌──────────────────────────────────────────────────┐       │
│  │              ingestion/parser.rs                  │       │
│  │  CodeAnalyzer - tree-sitter wrapper               │       │
│  │  - analyze_with_handler(source, handler)          │       │
│  │    -> Vec<(CodeChunk, Vec<String>)>               │       │
│  │  - extract_module_docs(source) -> Option<String>  │       │
│  │  parse_cargo_toml() - crate metadata              │       │
│  └──────┬───────────────────────────────────────────┘       │
│         │                                                    │
│         ▼                                                    │
│  ┌──────────────────────────────────────────────────┐       │
│  │              ingestion/reconcile.rs               │       │
│  │  reconcile(current, existing) -> ReconcileResult  │       │
│  │  Pure data comparison for incremental ingestion   │       │
│  │  File-level hash comparison: skip/replace/delete  │       │
│  └──────────────────────────────────────────────────┘       │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

#### code-rag-engine Components (Pure Algorithms)

```
┌─────────────────────────────────────────────────────────────┐
│                      code-rag-engine                         │
│          (no I/O, compiles to native + wasm32)               │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  ┌──────────────────────────────────────────────────┐       │
│  │                 intent.rs                         │       │
│  │  IntentClassifier — cosine similarity vs          │       │
│  │    pre-computed prototype embeddings              │       │
│  │  ::build(closure) — caller provides embed fn      │       │
│  │  ::from_prototypes() — load pre-computed          │       │
│  │  classify(query_embedding) -> ClassificationResult│       │
│  │  route(intent, table) -> RetrievalConfig          │       │
│  │  QueryIntent enum, RoutingTable                   │       │
│  │  impl FromStr for QueryIntent                     │       │
│  └──────────────────────────────────────────────────┘       │
│                                                              │
│  ┌──────────────────────────────────────────────────┐       │
│  │                 context.rs                        │       │
│  │  build_context(result) -> String                  │       │
│  │  build_prompt(query, context) -> String            │       │
│  │  SYSTEM_PROMPT constant                           │       │
│  └──────────────────────────────────────────────────┘       │
│                                                              │
│  ┌──────────────────────────────────────────────────┐       │
│  │                 config.rs                         │       │
│  │  EngineConfig { routing: RoutingTable }           │       │
│  │  RetrievalConfig { code/readme/crate/doc limits } │       │
│  └──────────────────────────────────────────────────┘       │
│                                                              │
│  ┌──────────────────────────────────────────────────┐       │
│  │                 retriever.rs                      │       │
│  │  ScoredChunk<T> — generic chunk + relevance score │       │
│  │  RetrievalResult — 4 scored chunk vectors + intent│       │
│  │  FlatChunk — type-erased chunk for evaluation     │       │
│  │  ::flatten() — single sorted list across types    │       │
│  │  distance_to_relevance(dist) -> f32               │       │
│  │  to_retrieval_result() — build from raw tuples    │       │
│  └──────────────────────────────────────────────────┘       │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

#### code-rag-chat Components (Root Crate — Web Server)

```
┌─────────────────────────────────────────────────────────────┐
│                      code-rag-chat                           │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  ┌──────────────┐                                           │
│  │   main.rs    │  Axum server setup, CORS, ServeDir        │
│  │              │  + SPA fallback for Leptos WASM            │
│  └──────┬───────┘                                           │
│         │                                                    │
│         ▼                                                    │
│  ┌───────────────────────────────────────────────────┐      │
│  │                    api/                            │      │
│  │  ┌────────────┐  ┌────────────┐  ┌────────────┐  │      │
│  │  │ handlers.rs│  │  state.rs  │  │   dto.rs   │  │      │
│  │  │ POST /chat │  │ AppState { │  │ ChatRequest│  │      │
│  │  │ GET /proj  │  │  embedder  │  │ ChatResp   │  │      │
│  │  │ GET /health│  │  classifier│  │ SourceInfo │  │      │
│  │  └────────────┘  │  store,llm │  └────────────┘  │      │
│  │  ┌────────────┐  │  config    │                   │      │
│  │  │  error.rs  │  │ }          │                   │      │
│  │  │  ApiError  │  └────────────┘                   │      │
│  │  └────────────┘                                   │      │
│  └───────────────────────────────────────────────────┘      │
│         │                                                    │
│         ▼                                                    │
│  ┌───────────────────────────────────────────────────┐      │
│  │          engine/ (re-exports from code-rag-engine) │      │
│  │  ┌────────────┐  ┌────────────┐                   │      │
│  │  │retriever.rs│  │generator.rs│                   │      │
│  │  │ retrieve() │  │ LlmClient  │                   │      │
│  │  │ scored srch│  │ Gemini API │                   │      │
│  │  └────────────┘  └────────────┘                   │      │
│  │  Re-exports: context, intent, EngineConfig,        │      │
│  │    RetrievalConfig, FlatChunk                      │      │
│  │  Local: EngineError (wraps embed/store/gen errors) │      │
│  └───────────────────────────────────────────────────┘      │
│                                                              │
│  ┌──────────────┐                                           │
│  │bin/harness.rs│  Quality measurement CLI                  │
│  │              │  code-rag-harness binary                  │
│  └──────┬───────┘                                           │
│         │                                                    │
│         ▼                                                    │
│  ┌───────────────────────────────────────────────────┐      │
│  │                    harness/                        │      │
│  │  dataset.rs    TestCase, TestDataset (JSON loader) │      │
│  │  runner.rs     Query execution (embed→classify→    │      │
│  │                route→retrieve, no LLM)            │      │
│  │  matching.rs   Hit detection (pure functions)      │      │
│  │  metrics.rs    recall@K, MRR, latency percentiles  │      │
│  │  report.rs     JSON + Markdown output              │      │
│  └───────────────────────────────────────────────────┘      │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

#### code-rag-ui Components (Leptos WASM Frontend)

```
┌─────────────────────────────────────────────────────────────┐
│                       code-rag-ui                            │
│              Leptos 0.8 CSR — trunk build                    │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  ┌──────────────┐                                           │
│  │   main.rs    │  Feature-gated entry point:                │
│  │              │  [default]    → backend_app()              │
│  │              │  [standalone] → standalone_app()           │
│  └──────┬───────┘                                           │
│         │                                                    │
│         ▼                                                    │
│  ┌───────────────────────────────────────────────────┐      │
│  │                 components/                        │      │
│  │  ┌────────────┐  ┌──────────────┐                 │      │
│  │  │ ChatView   │  │ SourcesPanel │  Shared UI      │      │
│  │  │ (messages, │  │ (relevance   │  components     │      │
│  │  │  input)    │  │  scores)     │                 │      │
│  │  └────────────┘  └──────────────┘                 │      │
│  │  ┌────────────┐  ┌──────────────┐                 │      │
│  │  │IntentBadge │  │  AuthPanel   │  (standalone    │      │
│  │  │            │  │  OAuth2+key  │   feature only) │      │
│  │  └────────────┘  └──────────────┘                 │      │
│  └───────────────────────────────────────────────────┘      │
│         │                                                    │
│  ┌──────┴──────────────────────────────────────┐            │
│  │             Data Layer (feature-gated)       │            │
│  │                                              │            │
│  │  [default]          │  [standalone]           │            │
│  │  ┌──────────┐       │  ┌──────────────────┐  │            │
│  │  │  api.rs  │       │  │standalone_api.rs │  │            │
│  │  │ fetch()  │       │  │ full + rag-only  │  │            │
│  │  │ → /chat  │       │  │ pipeline variants│  │            │
│  │  └──────────┘       │  ├──────────────────┤  │            │
│  │                     │  │ embedder.rs      │  │            │
│  │                     │  │ → transformers.js│  │            │
│  │                     │  ├──────────────────┤  │            │
│  │                     │  │ data.rs + search │  │            │
│  │                     │  │ ChunkIndex, L2   │  │            │
│  │                     │  ├──────────────────┤  │            │
│  │                     │  │ gemini.rs        │  │            │
│  │                     │  │ REST API client  │  │            │
│  │                     │  ├──────────────────┤  │            │
│  │                     │  │ auth.rs          │  │            │
│  │                     │  │ OAuth2 PKCE +    │  │            │
│  │                     │  │ API key          │  │            │
│  │                     │  └──────────────────┘  │            │
│  └──────────────────────────────────────────────┘            │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

---

## 4. Interface Contracts

### 4.1 code-rag-types — Domain Types

**Location:** [crates/code-rag-types/src/lib.rs](../crates/code-rag-types/src/lib.rs)

These types define the contract between producer (code-raptor) and consumer (code-rag-chat). They contain **no business logic**, only serde serialization.

**Helper functions** (also in `code-rag-types`):

```rust
/// Generate SHA256 hash of content.
/// Normalizes CRLF → LF before hashing for cross-OS consistency.
pub fn content_hash(content: &str) -> String;

/// Generate new UUID v4
pub fn new_chunk_id() -> String;

/// Deterministic chunk ID from file path + content.
/// Same function in same file = same ID across re-indexing runs.
/// Stable foreign key for Track C call graph edges.
pub fn deterministic_chunk_id(file_path: &str, content: &str) -> String;
```

**Chunk types:**

```rust
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CodeChunk {
    pub file_path: String,
    pub language: String,        // "rust", "python", "typescript"
    pub identifier: String,      // Function/class/struct/trait name
    pub node_type: String,       // Tree-sitter node type
    pub code_content: String,    // Complete source code of the element
    pub start_line: usize,       // 1-indexed line number
    pub project_name: String,    // e.g., "7_wonders", "catan"
    pub docstring: Option<String>, // Extracted /// / docstrings (V1.5)

    // V1.1 fields:
    pub chunk_id: String,        // Deterministic: hash(file_path, content)
    pub content_hash: String,    // SHA256 of source file for change detection
    pub embedding_model_version: String, // e.g., "BGESmallENV15_384"
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ReadmeChunk {
    pub file_path: String,
    pub project_name: String,
    pub content: String,
    pub chunk_id: String,
    pub content_hash: String,
    pub embedding_model_version: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CrateChunk {
    pub crate_name: String,
    pub crate_path: String,
    pub description: Option<String>,
    pub dependencies: Vec<String>,   // Workspace/local dependencies only
    pub project_name: String,
    pub chunk_id: String,
    pub content_hash: String,
    pub embedding_model_version: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ModuleDocChunk {
    pub file_path: String,
    pub module_name: String,
    pub doc_content: String,         // Concatenated //! doc comments
    pub project_name: String,
    pub chunk_id: String,
    pub content_hash: String,
    pub embedding_model_version: String,
}
```

**Key design notes:**
- All `project_name` fields are `String` (required), not `Option<String>`
- All chunk types carry `chunk_id`, `content_hash`, and `embedding_model_version` (V1.1)
- `chunk_id` uses `deterministic_chunk_id()` — stable across re-indexing for unchanged code
- `content_hash` enables incremental ingestion (V1.3): skip files whose hash hasn't changed
- `code-rag-types` also exports helper functions (`content_hash`, `new_chunk_id`, `deterministic_chunk_id`) but contains **no business logic**

### 4.2 code-rag-store — Embedding & Storage

**Location:** [crates/code-rag-store/src/](../crates/code-rag-store/src/)

#### Embedder (embedder.rs)

```rust
/// Error types for embedding operations.
#[derive(Error, Debug)]
pub enum EmbedError {
    /// Model initialization failed (e.g., download error, OOM)
    #[error("failed to initialize embedding model: {0}")]
    Init(#[from] anyhow::Error),

    /// Embedding generation failed for input text
    #[error("embedding generation failed: {0}")]
    Embed(String),
}

/// FastEmbed wrapper for text embeddings.
///
/// Holds loaded model weights in memory. Must be protected by Mutex
/// for concurrent access in the web server.
///
/// # Model
/// Default: BGE-small-en-v1.5 (384 dimensions)
/// - Good balance of quality and speed for code
/// - ~33M parameters, runs locally without GPU
///
/// # Thread Safety
/// The underlying model is NOT thread-safe. Wrap in `Mutex<Embedder>`
/// for concurrent HTTP handlers.
pub struct Embedder {
    model: TextEmbedding,
    dimension: usize,
}

impl Embedder {
    /// Initialize with default model (BGE-small-en-v1.5, 384 dimensions).
    ///
    /// Downloads model on first run (~50MB), cached in `.fastembed_cache/`.
    ///
    /// # Errors
    /// Returns `EmbedError::Init` if model download or loading fails.
    pub fn new() -> Result<Self, EmbedError>;

    /// Initialize with a specific FastEmbed model.
    ///
    /// # Supported Models
    /// - `BGESmallENV15` (384 dim) - default, recommended
    /// - `BGEBaseENV15` (768 dim) - higher quality, slower
    /// - `BGELargeENV15` (1024 dim) - highest quality, slowest
    /// - `AllMiniLML6V2` (384 dim) - alternative lightweight
    pub fn with_model(model_name: EmbeddingModel) -> Result<Self, EmbedError>;

    /// Embed a single text string.
    ///
    /// Convenience wrapper around `embed_batch` for single inputs.
    ///
    /// # Returns
    /// 384-dimensional vector (f32) for default model.
    pub fn embed_one(&mut self, text: &str) -> Result<Vec<f32>, EmbedError>;

    /// Embed multiple texts in a single call (more efficient).
    ///
    /// Use batch size of 25 for memory efficiency during ingestion.
    ///
    /// # Returns
    /// Vector of embeddings, one per input text.
    pub fn embed_batch(&mut self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError>;

    /// Get embedding dimension (384 for default model).
    pub fn dimension(&self) -> usize;
}
```

#### Text Formatting Functions (embedder.rs)

```rust
/// Formats a code chunk for embedding.
///
/// Concatenates identifier + language + docstring + code + calls for richer semantic signal.
///
/// # Output Format
/// ```text
/// identifier (language)
/// docstring (if present)
/// code_content
/// Calls: foo, bar (if non-empty, V2.1)
/// ```
///
/// # Example
/// ```text
/// retrieve (rust)
/// Embed the query text, search vector store for similar chunks
/// fn retrieve(...) { ... }
/// Calls: embed_one, search_all
/// ```
pub fn format_code_for_embedding(
    identifier: &str,
    language: &str,
    docstring: Option<&str>,
    code: &str,
    calls: &[String],  // V2.1: ephemeral call context, not stored on CodeChunk
) -> String;

/// Formats a README for embedding.
///
/// # Output Format
/// ```text
/// Project: project_name
/// content
/// ```
pub fn format_readme_for_embedding(project_name: &str, content: &str) -> String;

/// Formats a crate for embedding.
///
/// # Output Format
/// ```text
/// Crate: crate_name
/// description (if present)
/// Dependencies: dep1, dep2, dep3
/// ```
pub fn format_crate_for_embedding(
    crate_name: &str,
    description: Option<&str>,
    deps: &[String],
) -> String;

/// Formats module documentation for embedding.
///
/// # Output Format
/// ```text
/// Module: module_name
/// doc_content
/// ```
pub fn format_module_doc_for_embedding(module_name: &str, doc_content: &str) -> String;
```

#### VectorStore (vector_store.rs)

```rust
/// Error types for vector store operations.
#[derive(Error, Debug)]
pub enum StoreError {
    /// LanceDB operation failed
    #[error("database error: {0}")]
    Database(#[from] lancedb::Error),

    /// Arrow format conversion failed
    #[error("arrow error: {0}")]
    Arrow(#[from] arrow_schema::ArrowError),

    /// Requested table doesn't exist (no data ingested yet)
    #[error("table '{0}' not found")]
    TableNotFound(String),

    /// Column type mismatch during extraction
    #[error("schema mismatch: {0}")]
    SchemaMismatch(String),
}

/// LanceDB-backed vector store for code chunks.
///
/// Manages 4 tables, each with its own schema:
/// - `code_chunks` - function-level code
/// - `readme_chunks` - README files
/// - `crate_chunks` - Cargo.toml metadata
/// - `module_doc_chunks` - //! documentation
///
/// # Connection
/// Uses local file-based LanceDB. Creates database directory if not exists.
///
/// # Thread Safety
/// `Connection` is internally reference-counted and safe for concurrent use.
pub struct VectorStore {
    conn: Connection,
    dimension: usize,
}

impl VectorStore {
    /// Connect to LanceDB at the given path.
    ///
    /// Creates the database directory if it doesn't exist.
    /// Does NOT create tables - tables are created on first upsert.
    ///
    /// # Arguments
    /// * `db_path` - Path to LanceDB directory (e.g., "data/portfolio.lance")
    /// * `embedding_dimension` - Must match Embedder dimension (384 for BGE-small)
    pub async fn new(db_path: &str, embedding_dimension: usize) -> Result<Self, StoreError>;

    // ═══════════════════════════════════════════════════════════════
    // Write Operations (used by code-raptor)
    // ═══════════════════════════════════════════════════════════════

    /// Insert code chunks with their embeddings.
    ///
    /// Creates `code_chunks` table on first call. Subsequent calls append data.
    ///
    /// # Returns
    /// Number of chunks inserted.
    pub async fn upsert_code_chunks(
        &self,
        chunks: &[CodeChunk],
        embeddings: Vec<Vec<f32>>,
    ) -> Result<usize, StoreError>;

    /// Insert README chunks with their embeddings.
    pub async fn upsert_readme_chunks(
        &self,
        chunks: &[ReadmeChunk],
        embeddings: Vec<Vec<f32>>,
    ) -> Result<usize, StoreError>;

    /// Insert crate chunks with their embeddings.
    pub async fn upsert_crate_chunks(
        &self,
        chunks: &[CrateChunk],
        embeddings: Vec<Vec<f32>>,
    ) -> Result<usize, StoreError>;

    /// Insert module doc chunks with their embeddings.
    pub async fn upsert_module_doc_chunks(
        &self,
        chunks: &[ModuleDocChunk],
        embeddings: Vec<Vec<f32>>,
    ) -> Result<usize, StoreError>;

    // ═══════════════════════════════════════════════════════════════
    // Read Operations (used by code-rag-chat)
    // ═══════════════════════════════════════════════════════════════

    /// Search code chunks by vector similarity.
    ///
    /// Uses L2 (Euclidean) distance (LanceDB default).
    ///
    /// # Arguments
    /// * `query_embedding` - 384-dim query vector
    /// * `limit` - Maximum number of results (typically 5)
    pub async fn search_code(
        &self,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<CodeChunk>, StoreError>;

    /// Search README chunks by vector similarity.
    pub async fn search_readme(
        &self,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<ReadmeChunk>, StoreError>;

    /// Search crate chunks by vector similarity.
    pub async fn search_crates(
        &self,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<CrateChunk>, StoreError>;

    /// Search module doc chunks by vector similarity.
    pub async fn search_module_docs(
        &self,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<ModuleDocChunk>, StoreError>;

    /// Search all 4 tables and return combined results.
    ///
    /// This is the primary search method used by the retriever.
    ///
    /// # Returns
    /// Tuple of (code_chunks, readme_chunks, crate_chunks, module_doc_chunks)
    pub async fn search_all(
        &self,
        query_embedding: &[f32],
        code_limit: usize,
        readme_limit: usize,
        crate_limit: usize,
        module_doc_limit: usize,
    ) -> Result<(Vec<CodeChunk>, Vec<ReadmeChunk>, Vec<CrateChunk>, Vec<ModuleDocChunk>), StoreError>;

    /// List all unique project names in the database.
    ///
    /// Used by the `/projects` API endpoint.
    pub async fn list_projects(&self) -> Result<Vec<String>, StoreError>;
}
```

### 4.3 LanceDB Schema Contract

The LanceDB schema is the **sole coupling point** between code-raptor and code-rag-chat.

#### code_chunks Table

| Column | Type | Nullable | Purpose |
|--------|------|----------|---------|
| `file_path` | UTF8 | NO | Full path to source file |
| `language` | UTF8 | NO | "rust", "python", or "typescript" |
| `identifier` | UTF8 | NO | Function/class name |
| `node_type` | UTF8 | NO | Tree-sitter node type |
| `code_content` | UTF8 | NO | Source code snippet |
| `start_line` | UInt64 | NO | 1-indexed line number |
| `project_name` | UTF8 | NO | Parent project name |
| `docstring` | UTF8 | YES | Extracted documentation (V1.5) |
| `chunk_id` | UTF8 | NO | Deterministic ID: hash(file_path, content) |
| `content_hash` | UTF8 | NO | SHA256 of source file for change detection |
| `embedding_model_version` | UTF8 | NO | e.g., "BGESmallENV15_384" |
| `vector` | FixedSizeList(Float32, 384) | NO | Embedding vector |

#### readme_chunks Table

| Column | Type | Nullable |
|--------|------|----------|
| `file_path` | UTF8 | NO |
| `project_name` | UTF8 | NO |
| `content` | UTF8 | NO |
| `chunk_id` | UTF8 | NO |
| `content_hash` | UTF8 | NO |
| `embedding_model_version` | UTF8 | NO |
| `vector` | FixedSizeList(Float32, 384) | NO |

#### crate_chunks Table

| Column | Type | Nullable |
|--------|------|----------|
| `crate_name` | UTF8 | NO |
| `crate_path` | UTF8 | NO |
| `description` | UTF8 | YES |
| `dependencies` | List\<UTF8\> | NO | Array of workspace dependency names |
| `project_name` | UTF8 | NO |
| `chunk_id` | UTF8 | NO |
| `content_hash` | UTF8 | NO |
| `embedding_model_version` | UTF8 | NO |
| `vector` | FixedSizeList(Float32, 384) | NO |

#### module_doc_chunks Table

| Column | Type | Nullable |
|--------|------|----------|
| `file_path` | UTF8 | NO |
| `module_name` | UTF8 | NO |
| `doc_content` | UTF8 | NO |
| `project_name` | UTF8 | NO |
| `chunk_id` | UTF8 | NO |
| `content_hash` | UTF8 | NO |
| `embedding_model_version` | UTF8 | NO |
| `vector` | FixedSizeList(Float32, 384) | NO |

### 4.4 code-raptor — Ingestion Pipeline

**Location:** [crates/code-raptor/src/](../crates/code-raptor/src/)

#### LanguageHandler Trait (V1.2)

```rust
/// Trait for language-specific code parsing behavior.
///
/// Each supported language implements this trait. Adding a new language requires:
/// 1. Create a new handler struct (e.g., `GoHandler`)
/// 2. Implement 5 required methods
/// 3. Register in `handler_for_path()` registry
///
/// No enum, no match arms — fully open for extension.
pub trait LanguageHandler {
    /// Language identifier (e.g., "rust", "python", "typescript")
    fn name(&self) -> &'static str;

    /// File extensions this handler supports (e.g., &["rs"] for Rust)
    fn extensions(&self) -> &'static [&'static str];

    /// Get the tree-sitter grammar for this language
    fn grammar(&self) -> tree_sitter::Language;

    /// Tree-sitter S-expression query for extracting code elements.
    /// Must capture `@name` (identifier) and `@body` (full element node).
    fn query_string(&self) -> &'static str;

    /// Extract documentation from a code element.
    /// Default returns None. Per-language implementations added in V1.5.
    fn extract_docstring(
        &self, _source: &str, _node: &Node, _source_bytes: &[u8],
    ) -> Option<String> { None }

    /// Extract function/method call identifiers from a code element (V2.1).
    /// Default returns empty vec. Per-language implementations walk AST for
    /// call_expression / call nodes.
    fn extract_calls(
        &self, _source: &str, _node: &Node, _source_bytes: &[u8],
    ) -> Vec<String> { Vec::new() }
}

/// Registry: returns the appropriate handler for a file path based on extension.
pub fn handler_for_path(path: &Path) -> Option<Box<dyn LanguageHandler>>;
```

**Implementations:** `RustHandler`, `PythonHandler`, `TypeScriptHandler` (V1.4)

#### CodeAnalyzer (parser.rs)

```rust
/// Tree-sitter based code analyzer.
pub struct CodeAnalyzer {
    parser: Parser,
}

impl CodeAnalyzer {
    pub fn new() -> Self;

    /// Analyze source code using a LanguageHandler.
    ///
    /// # Returns
    /// Vector of (CodeChunk, calls) tuples. Calls are ephemeral — used to
    /// enrich embedding text, then discarded. No schema change.
    ///
    /// # Deduplication
    /// Results are deduplicated by (identifier, start_line).
    pub fn analyze_with_handler(
        &mut self, source: &str, handler: &dyn LanguageHandler,
    ) -> Vec<(CodeChunk, Vec<String>)>;

    /// Extract module-level //! documentation from Rust source.
    pub fn extract_module_docs(&mut self, source: &str) -> Option<String>;
}

/// Parse Cargo.toml and extract crate metadata.
pub fn parse_cargo_toml(content: &str) -> Option<(String, Option<String>, Vec<String>)>;
```

#### Incremental Ingestion (reconcile.rs, V1.3)

```rust
/// Compare current ingestion results against existing DB state.
/// Returns which chunks to insert, update, or delete.
///
/// Uses content_hash for file-level change detection:
/// - Unchanged hash → skip (no re-embedding)
/// - Changed hash → replace chunk
/// - Missing from current → delete from DB
pub fn reconcile(
    current: &IngestionResult,
    existing: &ExistingState,
) -> ReconcileResult;
```

#### Export Subcommand (export.rs)

```rust
/// Export all chunks with embeddings from LanceDB to a single JSON file.
///
/// Produces a `ChunkIndex` JSON containing:
/// - All 4 chunk types with their embedding vectors
/// - Pre-computed intent prototype embeddings
///
/// Used by GitHub Pages CI to generate static data for the standalone WASM demo.
///
/// # Usage
/// `code-raptor export --db-path data/portfolio.lance --output static/index.json`
pub async fn run_export(db_path: &str, output: &str) -> Result<()>;
```

### 4.5 code-rag-engine — Pure Algorithms

**Location:** [crates/code-rag-engine/src/](../crates/code-rag-engine/src/)

This crate contains all platform-agnostic RAG algorithms. It has **no I/O**, **no HTTP**, and compiles to both native and `wasm32-unknown-unknown`. Both Docker (code-rag-chat) and GitHub Pages (code-rag-ui standalone) use the same algorithms.

**Modules:** `intent`, `context`, `config`, `retriever`

#### IntentClassifier (intent.rs)

```rust
/// Query intent categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub enum QueryIntent { Overview, Implementation, Relationship, Comparison }

/// Pre-computed prototype embeddings for intent classification.
/// Classification uses cosine similarity against prototypes — no keywords.
pub struct IntentClassifier { /* prototype embeddings per intent */ }

impl IntentClassifier {
    /// Build classifier by embedding prototype queries.
    /// Caller provides their own embed function via closure.
    /// Native: passes fastembed. WASM: passes transformers.js.
    /// Export tool: pre-computes and serializes prototypes.
    pub fn build<E>(
        embed_batch: impl FnMut(&[&str]) -> Result<Vec<Vec<f32>>, E>,
    ) -> Result<Self, E>;

    /// Load from pre-computed prototype embeddings (WASM standalone mode).
    pub fn from_prototypes(
        prototypes: HashMap<QueryIntent, Vec<Vec<f32>>>,
    ) -> Self;
}

pub struct ClassificationResult {
    pub intent: QueryIntent,
    pub confidence: f32,  // Cosine similarity score
}

/// Classify using pre-computed query embedding (not raw text).
/// Embedding is reused for both classification and vector search.
pub fn classify(
    query_embedding: &[f32],
    classifier: &IntentClassifier,
) -> ClassificationResult;

/// Declarative routing table: maps each intent to retrieval limits.
pub struct RoutingTable {
    pub routes: HashMap<QueryIntent, RetrievalConfig>,
    pub default: RetrievalConfig,
}

pub fn route(intent: QueryIntent, table: &RoutingTable) -> RetrievalConfig;
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32;
```

#### Context Building (context.rs)

```rust
pub const SYSTEM_PROMPT: &str;
pub fn build_context(result: &RetrievalResult) -> String;
pub fn build_prompt(query: &str, context: &str) -> String;
```

#### Scored Retrieval (retriever.rs)

```rust
/// Generic wrapper pairing any chunk type with a relevance score.
pub struct ScoredChunk<T> { pub chunk: T, pub relevance: f32 }

/// All 4 chunk type results with scores and classified intent.
pub struct RetrievalResult {
    pub code_chunks: Vec<ScoredChunk<CodeChunk>>,
    pub readme_chunks: Vec<ScoredChunk<ReadmeChunk>>,
    pub crate_chunks: Vec<ScoredChunk<CrateChunk>>,
    pub module_doc_chunks: Vec<ScoredChunk<ModuleDocChunk>>,
    pub intent: QueryIntent,
}

pub fn distance_to_relevance(dist: f32) -> f32;  // 1.0 / (1.0 + dist)
pub fn to_retrieval_result(...) -> RetrievalResult;
```

#### Configuration (config.rs)

```rust
pub struct EngineConfig { pub routing: RoutingTable }
pub struct RetrievalConfig {
    pub code_limit: usize,
    pub readme_limit: usize,
    pub crate_limit: usize,
    pub module_doc_limit: usize,
}
```

### 4.6 code-rag-chat — Query Interface (Root Crate)

**Location:** [src/](../src/)

The root crate's `engine/` module re-exports from `code-rag-engine` and adds I/O-bound components (scored LanceDB search, Gemini API client). Only `retriever.rs` and `generator.rs` are local; everything else comes from `code-rag-engine`.

#### HTTP API Contract

| Endpoint | Method | Request | Response |
|----------|--------|---------|----------|
| `/api/chat` | POST | `{"query": string}` | `{"answer": string, "sources": [...], "intent": "..."}` |
| `/api/projects` | GET | - | `{"projects": [string], "count": number}` |
| `/health` | GET | - | `{"status": "ok"}` |

#### Engine Components (re-exports + local)

```rust
// Re-exports from code-rag-engine
pub use code_rag_engine::context;
pub use code_rag_engine::intent;
pub use code_rag_engine::config::EngineConfig;

// Local: I/O-bound components
pub mod retriever;   // Scored search from LanceDB
pub mod generator;   // LlmClient (Gemini API via rig-core)

/// Pipeline error type (wraps embed, store, and generation errors)
pub enum EngineError { ... }
```

```rust
/// RAG pipeline configuration.
/// EngineConfig now lives in code-rag-engine::config.
/// Contains only RoutingTable (intent classification moved to IntentClassifier).
#[derive(Clone, Debug)]
pub struct EngineConfig {
    pub routing: RoutingTable,        // intent → retrieval config mapping
}

// RetrievalConfig, RetrievalResult, SYSTEM_PROMPT, build_context(),
// build_prompt() — see Section 4.5 (code-rag-engine)

/// Execute scored retrieval pipeline (local to code-rag-chat).
///
/// 1. Search all 4 LanceDB tables with pre-computed query embedding
/// 2. Convert distances to relevance scores
/// 3. Return RetrievalResult with ScoredChunk<T> wrappers
///
/// Query embedding is computed once in the handler and reused
/// for both intent classification and retrieval.
pub async fn retrieve(
    query_embedding: &[f32],
    store: &VectorStore,
    config: &RetrievalConfig,
    intent: QueryIntent,
) -> Result<RetrievalResult, EngineError>;
```

### 4.7 code-rag-ui — Frontend

**Location:** [crates/code-rag-ui/](../crates/code-rag-ui/)

Leptos 0.8 CSR app built with `trunk`. Feature-gated for two deployment modes:

| Mode | Feature Flag | Data Source | Embedding | LLM |
|------|-------------|-------------|-----------|-----|
| **Default** | (none) | HTTP API → code-rag-chat | Server-side (FastEmbed) | Server-side (Gemini) |
| **Standalone** | `--features standalone` | Static `index.json` | In-browser (transformers.js) | Optional (Gemini REST, requires auth) |

**Shared components** (both modes): `ChatView`, `SourcesPanel`, `IntentBadge`, `ProjectTags`, `ThemeToggle`

**Standalone-only modules:**
- `embedder.rs` — `#[wasm_bindgen]` bridge to `window.__codeRagEmbedQuery()` (transformers.js)
- `data.rs` — `ChunkIndex` type, `load_index()` fetches pre-computed JSON from static assets
- `search.rs` — Brute-force L2 vector search over `EmbeddedChunk<T>` vectors
- `gemini.rs` — Direct Gemini REST API client (supports `AuthMethod::ApiKey` and `AuthMethod::OAuth2`)
- `auth.rs` — OAuth2 PKCE flow (code verifier, SHA-256 challenge, token exchange), localStorage persistence
- `standalone_api.rs` — Two pipeline variants:
  - `send_chat_standalone()` — Full pipeline with Gemini LLM
  - `send_chat_rag_only()` — Retrieval without LLM (works unauthenticated)

**Static assets:**
- `static/embedder.js` — Thin transformers.js v3.8.1 wrapper (CDN-loaded, model cached in IndexedDB ~33MB)
- `index.html` — Trunk build entry point

---

## 5. Configuration Schema

### 5.1 Current State (Hardcoded)

Currently, configuration values are hardcoded in [crates/code-rag-engine/src/config.rs](../crates/code-rag-engine/src/config.rs):

```rust
impl Default for RetrievalConfig {
    fn default() -> Self {
        Self {
            code_limit: 5,      // Hardcoded
            readme_limit: 2,    // Hardcoded
            crate_limit: 3,     // Hardcoded
            module_doc_limit: 3, // Hardcoded
        }
    }
}
```

Other hardcoded values:
- Embedding model: `BGESmallENV15` in `code-rag-store/src/embedder.rs`
- Batch size: `25` in `code-raptor/src/main.rs`
- Ignored directories: `target`, `.git`, `node_modules` in ingestion
- LLM model: `gemini-2.0-flash` in environment variable

### 5.2 Proposed Declarative Configuration

To enforce the **Declarative** principle, migrate to TOML configuration:

```toml
# config/coderag.toml (proposed)

[embedding]
model = "BGESmallENV15"       # Options: BGESmallENV15, BGEBaseENV15, BGELargeENV15
dimension = 384                # Auto-set based on model
batch_size = 25                # Chunks per embedding batch

[retrieval]
code_limit = 5
readme_limit = 2
crate_limit = 3
module_doc_limit = 3

# V2.3 intent routing (currently in-code via RoutingTable)
# Future: load from config file
[retrieval.intent_routing.overview]
code = 2
readme = 3
crate = 4
module_doc = 3
[retrieval.intent_routing.implementation]
code = 7
readme = 1
crate = 1
module_doc = 2
[retrieval.intent_routing.relationship]
code = 6
readme = 1
crate = 2
module_doc = 2
[retrieval.intent_routing.comparison]
code = 4
readme = 2
crate = 3
module_doc = 2

[ingestion]
ignored_dirs = [".git", "target", "node_modules", "__pycache__", ".venv"]
supported_languages = ["rust", "python", "typescript"]

[llm]
model = "gemini-2.0-flash"
system_prompt_file = "prompts/system.txt"  # Externalize prompt

[database]
path = "data/portfolio.lance"
```

### 5.3 Environment Variables

Current environment configuration (`.env`):

| Variable | Purpose | Required |
|----------|---------|----------|
| `GEMINI_API_KEY` | Google Gemini API authentication | Yes (Docker) |
| `DATABASE_PATH` | LanceDB storage location | No (default: `data/portfolio.lance`) |
| `PORT` | HTTP server port | No (default: `3000`) |
| `UI_DIST` | Path to trunk build output for Leptos WASM | No (default: `crates/code-rag-ui/dist`) |
| `GOOGLE_OAUTH_CLIENT_ID` | OAuth2 client ID for GitHub Pages standalone auth | No (standalone only, injected at build time) |

---

## 6. Extension Points

### 6.1 Adding New Languages

**Location:** [crates/code-raptor/src/ingestion/languages/](../crates/code-raptor/src/ingestion/languages/)

Since V1.2, language support uses the `LanguageHandler` trait — no enum, no match arms. Adding a new language is fully additive:

```rust
// Step 1: Create new handler file (e.g., languages/go.rs)
pub struct GoHandler;

impl LanguageHandler for GoHandler {
    fn name(&self) -> &'static str { "go" }

    fn extensions(&self) -> &'static [&'static str] { &["go"] }

    fn grammar(&self) -> tree_sitter::Language {
        tree_sitter_go::LANGUAGE.into()
    }

    fn query_string(&self) -> &'static str {
        r#"(function_declaration name: (identifier) @name) @body
(method_declaration name: (field_identifier) @name) @body"#
    }

    fn extract_docstring(
        &self, source: &str, node: &Node, source_bytes: &[u8],
    ) -> Option<String> {
        // Language-specific doc extraction
    }
}

// Step 2: Register in language.rs
pub fn handler_for_path(path: &Path) -> Option<Box<dyn LanguageHandler>> {
    let ext = path.extension()?.to_str()?;
    match ext {
        "rs" => Some(Box::new(RustHandler)),
        "py" => Some(Box::new(PythonHandler)),
        "ts" | "tsx" | "jsx" => Some(Box::new(TypeScriptHandler)),
        "go" => Some(Box::new(GoHandler)),  // Add one line
        _ => None,
    }
}

// Step 3: Add tree-sitter grammar to Cargo.toml
// [dependencies]
// tree-sitter-go = "0.23"
```

**No existing code changes needed** — fully additive. Existing handlers and tests are unaffected.

### 6.2 Adding New Chunk Types (A1: Hierarchy)

**Pattern for adding FolderChunk, FileChunk:**

1. **Define type** in `code-rag-types/src/lib.rs`:
   ```rust
   pub struct FolderChunk {
       pub folder_path: String,
       pub summary: String,  // Auto-generated description
       pub file_count: usize,
       pub project_name: Option<String>,
   }
   ```

2. **Add formatting** in `code-rag-store/src/embedder.rs`:
   ```rust
   pub fn format_folder_for_embedding(path: &str, summary: &str) -> String {
       format!("Folder: {}\n{}", path, summary)
   }
   ```

3. **Add schema** in `code-rag-store/src/vector_store.rs`:
   - Add `FOLDER_TABLE` constant
   - Implement `folder_chunks_to_batch()`
   - Implement `extract_folder_chunks_from_batch()`
   - Add `upsert_folder_chunks()` and `search_folders()`

4. **Add extraction** in `code-raptor/src/ingestion/`:
   - Add folder summarization logic
   - Update `run_ingestion()` to process folders

5. **Update algorithms** in `code-rag-engine/src/`:
   - Add to `RetrievalResult` in `retriever.rs`
   - Add `format_folder_section()` in `context.rs`

6. **Update retrieval** in `src/engine/retriever.rs` (code-rag-chat):
   - Add to `search_all()` call
   - Wire scored results into `to_retrieval_result()`

### 6.3 Intent Classification & Routing (V2.2 + V2.3)

**Location:** `crates/code-rag-engine/src/intent.rs`

Classification uses **cosine similarity** against pre-computed prototype query embeddings — no keyword rules. Routing maps the classified intent to retrieval limits via a declarative `RoutingTable`.

```rust
/// Query intent categories. Derives Hash + Eq for use as RoutingTable key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryIntent {
    Overview,        // "What is X?", "Tell me about Y"
    Implementation,  // "How does X work?", "Show me the code"
    Relationship,    // "What calls X?", "How does A relate to B?"
    Comparison,      // "Compare A and B", "Differences between X and Y"
}

/// Pre-computed prototype embeddings per intent.
/// Built at startup via closure (native: fastembed, WASM: pre-computed).
pub struct IntentClassifier { /* HashMap<QueryIntent, Vec<Vec<f32>>> */ }

pub struct ClassificationResult {
    pub intent: QueryIntent,
    pub confidence: f32,  // Cosine similarity score
}

/// Classify using pre-computed query embedding (reused for vector search).
pub fn classify(query_embedding: &[f32], classifier: &IntentClassifier) -> ClassificationResult;

/// Declarative routing table: maps each intent to retrieval limits.
/// Data, not code. New intents = new entries.
pub struct RoutingTable {
    pub routes: HashMap<QueryIntent, RetrievalConfig>,
    pub default: RetrievalConfig,  // Fallback: RetrievalConfig::default() (5/2/3/3)
}

/// Look up retrieval limits for a classified intent.
pub fn route(intent: QueryIntent, table: &RoutingTable) -> RetrievalConfig;
```

**Adding a new intent** requires:
1. New `QueryIntent` variant (one line)
2. New prototype queries in `IntentClassifier::build()` (one block of example queries)
3. New entry in `RoutingTable::default()` (one `routes.insert()` call)

No existing code changes needed — fully additive.

### 6.4 Hybrid Search (B1)

**Extension point:** `code-rag-store/src/vector_store.rs`

```rust
/// Search with combined BM25 + vector similarity.
///
/// Uses Reciprocal Rank Fusion (RRF) to merge results.
///
/// # Future Implementation
/// LanceDB supports full-text search. Add BM25 index to tables,
/// then fuse with vector results.
pub async fn search_hybrid(
    &self,
    query_text: &str,
    query_embedding: &[f32],
    limit: usize,
    bm25_weight: f32,  // 0.0-1.0, higher = more lexical
) -> Result<Vec<CodeChunk>, StoreError>;
```

### 6.5 Call Graph (C1-C5)

**Future table schema for `call_edges`:**

| Column | Type | Purpose |
|--------|------|---------|
| `caller_id` | UTF8 | Composite key: `file_path::identifier` |
| `callee_id` | UTF8 | Composite key: `file_path::identifier` |
| `edge_type` | UTF8 | `same_file`, `cross_file`, `module_import` |
| `project_name` | UTF8 | For scoping |

**Query interface:**
```rust
/// Find all callers of a function.
pub async fn find_callers(
    &self,
    callee_id: &str,
) -> Result<Vec<CallEdge>, StoreError>;

/// Find call chain from entry point to target.
pub async fn trace_call_path(
    &self,
    from_id: &str,
    to_id: &str,
) -> Result<Vec<CallEdge>, StoreError>;
```

---

## 7. Architectural Decision Records (ADRs)

### ADR-001: Workspace Structure with 6 Crates

**Status:** Accepted (updated from original 4-crate design)

**Context:**
The system has three distinct execution contexts with different constraints:
- Ingestion: Batch processing on code changes
- Query serving: Real-time HTTP requests
- Browser demo: Fully static, runs in WASM

These need to share algorithms and type definitions without coupling their runtimes or I/O.

**Decision:**
Organize as a Cargo workspace with 6 crates:
- `code-raptor` - Ingestion CLI (independent binary)
- `code-rag-chat` - Query API server (root crate, independent binary)
- `code-rag-engine` - Pure algorithms (no I/O, compiles to native + wasm32)
- `code-rag-ui` - Leptos WASM frontend (feature-gated: default API client or standalone in-browser RAG)
- `code-rag-store` - Shared embedding + storage
- `code-rag-types` - Shared type definitions (no logic)

**Consequences:**
- (+) Clear SoC: each crate has one responsibility
- (+) Independent update frequencies
- (+) Can compile and test crates independently
- (+) `code-rag-engine` ensures algorithm consistency across Docker and WASM
- (+) Feature-gated UI avoids separate crate for standalone mode
- (-) More boilerplate (6 Cargo.toml files)
- (-) Must be careful not to leak implementation details through types crate

---

### ADR-002: LanceDB as Sole Coupling Point

**Status:** Accepted

**Context:**
Producer (code-raptor) and consumer (code-rag-chat) need to exchange data without code-level coupling.

**Decision:**
All data exchange happens via LanceDB schema. The schema (4 tables with defined columns) is the contract. Neither crate imports the other.

**Consequences:**
- (+) No shared runtime state
- (+) Producer and consumer can run on different machines
- (+) Schema changes are explicit and versioned
- (+) Enables future multi-producer scenarios (CI/CD workers)
- (-) Schema is a critical interface - changes require coordination
- (-) No compile-time checks for schema compatibility

---

### ADR-003: Function-Level Chunking Granularity

**Status:** Accepted

**Context:**
Need to choose the semantic unit for embedding. Options:
- File-level: Coarse, loses precision
- Function-level: Balanced
- Line/block-level: Too fine, loses context

**Decision:**
1 function/class/struct/trait = 1 vector. Each named code element is a separate chunk.

**Consequences:**
- (+) Precise retrieval for "how does X work?" queries
- (+) Natural semantic boundary
- (+) Matches how developers think about code
- (-) Large functions may exceed context limits (planned: B3 chunking)
- (-) Cannot search within a function

---

### ADR-004: FastEmbed BGE-small-en-v1.5 for Embeddings

**Status:** Accepted

**Context:**
Need embedding model that:
- Runs locally (no API costs)
- Works well for code semantics
- Has reasonable memory footprint

**Decision:**
Use BGE-small-en-v1.5 via FastEmbed:
- 384 dimensions
- ~33M parameters
- Good code semantic capture
- ~50MB download, cached locally

**Consequences:**
- (+) No API costs or rate limits
- (+) Works offline
- (+) Consistent embeddings across runs
- (-) Quality below larger models (BGE-large, OpenAI)
- (-) First run requires model download

---

### ADR-005: Mutex on Embedder Only

**Status:** Accepted

**Context:**
Concurrent HTTP requests need shared access to resources:
- Embedder (model weights)
- VectorStore (database connection)
- LlmClient (API client)

**Decision:**
Only `Embedder` requires `Mutex`. VectorStore's `Connection` is internally reference-counted. LlmClient is stateless. IntentClassifier is immutable (pre-computed embeddings).

```rust
pub struct AppState {
    pub embedder: Mutex<Embedder>,    // Requires locking
    pub classifier: IntentClassifier, // Immutable, pre-computed at startup
    pub store: VectorStore,            // Internally Arc'd
    pub llm: LlmClient,                // Stateless
    pub config: EngineConfig,          // Immutable
}
```

**Consequences:**
- (+) Minimal lock contention
- (+) IntentClassifier built before Mutex wrap — no lock needed for classification
- (+) Most operations don't require locking
- (-) Embedder becomes bottleneck under high concurrency
- (Future) Could pool multiple Embedder instances

---

### ADR-006: Tree-sitter for Language Parsing

**Status:** Accepted

**Context:**
Need robust, multi-language AST parsing for code extraction. Options:
- Regex: Fragile, no semantic understanding
- Language-specific parsers: High maintenance
- Tree-sitter: Universal, grammar-based

**Decision:**
Use tree-sitter with per-language grammars:
- Declarative S-expression queries
- Consistent API across languages
- Easy to add new languages

**Consequences:**
- (+) Robust parsing (handles partial/invalid syntax)
- (+) Declarative query patterns
- (+) Large ecosystem of grammars
- (-) Dependency on external grammar crates
- (-) S-expression syntax has learning curve

---

### ADR-007: Leptos WASM Frontend

**Status:** Accepted

**Context:**
Needed a frontend that:
- Compiles to WASM for GitHub Pages static demo
- Produces small bundles (~100-300KB gzipped)
- Maintains architectural coherence (Rust end-to-end)
- Replaced htmx/Askama server-rendered approach

Options: Leptos, Yew, Dioxus, or JS framework (React/Svelte).

**Decision:**
Leptos 0.8 with CSR-only (no SSR). Feature flag `standalone` switches the data layer from HTTP API to in-browser pipeline. All UI components are shared between modes.

**Consequences:**
- (+) Smallest WASM bundles (fine-grained reactivity, no virtual DOM)
- (+) One language (Rust) for entire stack
- (+) Feature flag avoids separate crate for standalone mode
- (+) Visual theme consistent with paulxie Astro portfolio
- (-) Leptos ecosystem smaller than React
- (-) CSR-only means no SEO (acceptable for demo)

---

### ADR-008: Shared Engine Crate (code-rag-engine)

**Status:** Accepted

**Context:**
Intent classification, context building, and scoring logic was in `src/engine/` (server-only). GitHub Pages demo needed the same algorithms in WASM. Options:
- Duplicate logic in UI crate
- Extract shared crate with trait abstractions
- Extract shared crate with pure functions + closures

**Decision:**
Extract `code-rag-engine` as a pure Rust crate with no I/O. `IntentClassifier::build()` takes a closure (`impl FnMut(&[&str]) -> Result<Vec<Vec<f32>>, E>`) instead of a concrete `Embedder` type. `from_prototypes()` allows loading pre-computed embeddings in WASM.

**Consequences:**
- (+) Algorithms compile to both native and wasm32 unchanged
- (+) No trait overhead — closures are simpler than trait objects for one call site
- (+) Improvements to classification/context/scoring automatically apply to both deployments
- (+) Export tool pre-computes prototypes for WASM, avoiding runtime embedding cost
- (-) `src/engine/` becomes a thin re-export layer (acceptable trade-off)

---

## 8. Data Flow Diagrams

### 8.1 Ingestion Pipeline

```
┌──────────────────────────────────────────────────────────────────┐
│                     INGESTION PIPELINE                            │
└──────────────────────────────────────────────────────────────────┘

[Repository Path]
       │
       ▼
┌──────────────────┐
│  walkdir + filter │  Ignore: .git, target, node_modules
└────────┬─────────┘
         │
         ├────────────────────────────────────────────────────────┐
         │                                                         │
         ▼                                                         ▼
┌─────────────────────┐  ┌─────────────────────┐  ┌─────────────────────┐
│  is_readme?         │  │  is_cargo_toml?     │  │  handler_for_path() │
│  README.md          │  │  Cargo.toml         │  │  .rs, .py, .ts/tsx  │
└────────┬────────────┘  └────────┬────────────┘  └────────┬────────────┘
         │                        │                         │
         ▼                        ▼                         ▼
┌─────────────────────┐  ┌─────────────────────┐  ┌─────────────────────┐
│  ReadmeChunk        │  │  parse_cargo_toml() │  │  CodeAnalyzer       │
│  - file_path        │  │  -> CrateChunk      │  │  .analyze_with_     │
│  - project_name     │  │                     │  │   handler()         │
│  - content          │  │  + extract_module   │  │  -> Vec<CodeChunk>  │
└────────┬────────────┘  │  _docs() for lib.rs │  └────────┬────────────┘
         │               └────────┬────────────┘           │
         │                        │                         │
         └────────────────────────┼─────────────────────────┘
                                  │
                                  ▼
                    ┌──────────────────────────┐
                    │  format_*_for_embedding  │
                    │  (per chunk type)        │
                    └────────────┬─────────────┘
                                 │
                                 ▼
                    ┌──────────────────────────┐
                    │  Embedder.embed_batch    │
                    │  (batch_size: 25)        │
                    │  -> Vec<Vec<f32>>        │
                    └────────────┬─────────────┘
                                 │
                                 ▼
                    ┌──────────────────────────┐
                    │  VectorStore.upsert_*    │
                    │  (chunks + embeddings)   │
                    └────────────┬─────────────┘
                                 │
                                 ▼
                    ┌──────────────────────────┐
                    │       LanceDB            │
                    │  - code_chunks           │
                    │  - readme_chunks         │
                    │  - crate_chunks          │
                    │  - module_doc_chunks     │
                    └──────────────────────────┘
```

### 8.2 Query Pipeline

```
┌──────────────────────────────────────────────────────────────────┐
│                       QUERY PIPELINE                              │
└──────────────────────────────────────────────────────────────────┘

[User Query]  "How does the retriever work?"
       │
       ▼
┌──────────────────────────────────┐
│  POST /api/chat                  │
│  handlers::chat(query)           │
└────────────────┬─────────────────┘
                 │
                 ▼
┌──────────────────────────────────┐
│  Acquire Mutex<Embedder>         │
│  embedder.embed_one(query)       │
│  -> query_embedding [384]        │
│  (embedding reused for BOTH      │
│   classification and retrieval)  │
└────────────────┬─────────────────┘
                 │
                 ▼
┌──────────────────────────────────┐
│  classify(query_embedding,       │  (V2.2 — cosine similarity)
│           &classifier)           │
│  -> ClassificationResult {       │
│       intent: Implementation,    │
│       confidence: 0.82           │
│     }                            │
└────────────────┬─────────────────┘
                 │
                 ▼
┌──────────────────────────────────┐
│  route(intent, &routing_table)   │  (V2.3)
│  -> RetrievalConfig {            │
│       code_limit: 7,             │
│       readme_limit: 1,           │
│       crate_limit: 1,            │
│       module_doc_limit: 2        │
│     }                            │
└────────────────┬─────────────────┘
                 │
                 ▼
┌──────────────────────────────────┐
│  VectorStore.search_all(         │
│    query_embedding,              │
│    routed config limits          │
│  )                               │
└────────────────┬─────────────────┘
                 │
                 ▼
┌──────────────────────────────────┐
│  RetrievalResult {               │
│    code_chunks: [ScoredChunk],   │
│    readme_chunks: [ScoredChunk], │
│    crate_chunks: [ScoredChunk],  │
│    module_doc_chunks: [Scored],  │
│    intent: Implementation        │
│  }                               │
└────────────────┬─────────────────┘
                 │
                 ▼
┌──────────────────────────────────┐
│  context::build_context(result)  │
│                                  │
│  ## Crate Structure              │
│  ## Module Documentation         │
│  ## Relevant Code                │  <- Markdown formatted
│  ## Project Documentation        │
└────────────────┬─────────────────┘
                 │
                 ▼
┌──────────────────────────────────┐
│  context::build_prompt(          │
│    query,                        │
│    context                       │
│  )                               │
│                                  │
│  {SYSTEM_PROMPT}                 │
│  ---                             │
│  {context}                       │
│  ---                             │
│  **Question:** {query}           │
└────────────────┬─────────────────┘
                 │
                 ▼
┌──────────────────────────────────┐
│  generator::generate(prompt)     │
│  -> Gemini API call              │
│  -> ~2-5s latency                │
└────────────────┬─────────────────┘
                 │
                 ▼
┌──────────────────────────────────┐
│  ChatResponse {                  │
│    answer: "The retriever...",   │
│    sources: [...],               │
│    intent: "implementation"      │
│  }                               │
└──────────────────────────────────┘
```

---

## 9. Quality Attributes

### 9.1 Testability Strategy

| Level | What | How | Location |
|-------|------|-----|----------|
| **Unit tests** | Pure functions, parsers | Standard `#[test]` | Each crate's `mod tests` |
| **Integration tests** | End-to-end with fixtures | `#[ignore]` for I/O tests | `tests/` directories |
| **Hero queries** | Manual validation | 5-10 key queries per milestone | V1, V2 checkpoints |
| **Quantitative harness** | Automated recall@K | JSON test dataset + script | V3 infrastructure |

### 9.2 Performance Characteristics

| Operation | Typical Latency | Bottleneck |
|-----------|-----------------|------------|
| Embedding (single query) | ~50ms | CPU inference |
| Embedding (batch of 25) | ~200ms | CPU inference |
| Vector search (per table) | ~10ms | LanceDB I/O |
| Context building | <1ms | String formatting |
| LLM generation | 2-5s | Gemini API |
| **Full query (end-to-end)** | **3-6s** | **Dominated by LLM** |

### 9.3 Error Handling Pattern

```
┌─────────────────────────────────────────────────────────────────┐
│                    ERROR HANDLING LAYERS                         │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │  Crate-specific errors (thiserror)                        │   │
│  │  - EmbedError (embedding failures)                        │   │
│  │  - StoreError (database failures)                         │   │
│  │  - EngineError (pipeline failures)                        │   │
│  └────────────────────────┬─────────────────────────────────┘   │
│                           │                                      │
│                           ▼                                      │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │  Application errors (anyhow)                              │   │
│  │  - Used in main.rs for startup failures                   │   │
│  │  - Wraps crate errors with context                        │   │
│  └────────────────────────┬─────────────────────────────────┘   │
│                           │                                      │
│                           ▼                                      │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │  HTTP errors (ApiError)                                   │   │
│  │  - Maps to HTTP status codes                              │   │
│  │  - 400: Bad request (invalid query)                       │   │
│  │  - 500: Internal error (embed/store/LLM failures)         │   │
│  │  - 503: Service unavailable (model not loaded)            │   │
│  └──────────────────────────────────────────────────────────┘   │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

---

## 10. Appendices

### A. Glossary

| Term | Definition |
|------|------------|
| **Chunk** | Semantic unit of code/documentation for embedding (function, README, etc.) |
| **Embedding** | 384-dimensional vector representation of text |
| **Retrieval** | Vector similarity search to find relevant chunks |
| **Context** | Formatted chunks provided to LLM as reference material |
| **Intent** | Classified query type for routing (overview, implementation, etc.) |
| **RAPTOR** | Recursive Abstractive Processing for Tree-Organized Retrieval |

### B. References

- [development_plan.md](../development_plan.md) - V1-V3 roadmap + Tracks A/B/C
- [project-vision.md](../project-vision.md) - Full improvement ideas and differentiation
- [development_log.md](../development_log.md) - Version history (V0.1 through GitHub Pages demo)
- [LanceDB Documentation](https://lancedb.github.io/lancedb/)
- [FastEmbed](https://github.com/qdrant/fastembed)
- [tree-sitter](https://tree-sitter.github.io/tree-sitter/)

### C. Document Change Log

| Version | Date | Changes |
|---------|------|---------|
| 0.1 | 2026-02-01 | Initial architecture document |
| 0.2 | 2026-02-07 | V1 completion: updated coderag-types with V1.1 fields (chunk_id, content_hash, embedding_model_version), fixed project_name to String, updated LanceDB schema (List\<UTF8\> dependencies, V1.1 columns), replaced SupportedLanguage enum with LanguageHandler trait (V1.2), added reconcile.rs for incremental ingestion (V1.3), added TypeScript support (V1.4), updated docstring extraction (V1.5). V2 design: added intent classification + routing to engine components (V2.2/V2.3), updated query pipeline with classify → route steps, updated format_code_for_embedding with calls parameter (V2.1), updated EngineConfig with intent + routing fields. |
| 0.3 | 2026-03-29 | Major restructuring: updated from 4-crate to 6-crate workspace. Added `code-rag-engine` (pure algorithms, wasm32-compatible) and `code-rag-ui` (Leptos WASM frontend). Replaced htmx/Askama with Leptos SPA. Updated all crate names (`portfolio-rag-chat` → `code-rag-chat`, `coderag-store` → `code-rag-store`, `coderag-types` → `code-rag-types`). Updated intent classification from keyword-based to embedding-based (cosine similarity). Added GitHub Pages standalone deployment mode. Added ADR-007 (Leptos), ADR-008 (shared engine crate). Updated all diagrams, component layouts, AppState, query pipeline, and extension points. |
