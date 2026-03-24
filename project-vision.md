# CodeRAG Chatbot: Project Ideation

## Vision
A **generalizable code understanding system** split into two projects:

| Project | Purpose | Focus |
|---------|---------|-------|
| **code-raptor** | Build code knowledge graphs | Indexing, enrichment, topology |
| **portfolio-rag-chat** | Query interface | Intent routing, search, chat |

## Use Cases
- Works on any codebase (not just well-documented code)
- Answers questions about unfamiliar/third-party code
- Supports multi-repo ingestion
- Serves as both portfolio demo AND reusable tools

---

## Improvement Ideas

### 1. Hierarchical Embedding (folder → file → function)

| Verdict | **Yes - with intent routing** |
|---------|-------------------------------|

**Each level serves distinct query types:**

| Level | Answers | Example queries |
|-------|---------|-----------------|
| Folder | Subsystem/component overview | "What does the engine/ folder do?" |
| File/Module | Module responsibility, public API | "What's the purpose of this module?" |
| Function | Implementation details | "How does this function work?" |

**For unfamiliar codebases, architecture queries come first:**
- "How is this organized?"
- "What are the main components?"
- "Where's the entry point?"

**Critical prerequisite: Intent classification**
- Without routing → mixed granularity results, noisy
- With routing → each level retrieves what it's good at

**Skip block-level:**
- Poorly defined semantically (what's a meaningful block?)
- Handle large functions via chunking, not hierarchy

**Implementation approach:**
1. Add `FolderChunk` - auto-summarize folder contents (file list + inferred purpose)
2. Add `FileChunk` - module-level summary (exports, main responsibilities)
3. Keep `CodeChunk` as function-level
4. Build intent classifier to route queries to appropriate level
5. Large function chunking as separate concern

---

### 2. Call Graph Extraction

| Verdict | **Yes - but implement incrementally** |
|---------|---------------------------------------|

**Problems with full call graph:**
- Cross-file import resolution is complex
- Rust traits/generics, Python duck typing → incomplete static analysis
- High storage/maintenance overhead

**Call graphs serve THREE purposes:**

| Purpose | Query types | What it reveals |
|---------|-------------|-----------------|
| **Implementation** | "What calls X?", "Show the auth flow" | Function-level relationships |
| **Architecture** | "How do modules interact?", "What depends on what?" | System structure, coupling |
| **Debugging** | "How does error reach here?", "Trace execution path" | Error propagation, call chains |

**Architectural insights from call graphs:**
- Module-to-module dependencies → layering, coupling
- Central functions (high in-degree) → critical components to understand first
- Cross-folder calls → subsystem boundaries
- Import patterns → where abstractions leak

**Debugging insights from call graphs:**
- Reverse call chains → "how did we get here?"
- Error propagation paths → which callers are affected by a bug
- Entry points → where to start tracing from
- Leaf functions → side-effect boundaries

**Ties to hierarchical embedding:**
- Function calls → function-level graph
- Module imports → file/module-level graph
- Folder dependencies → subsystem-level graph

**Incremental implementation path:**

| Phase | Effort | Value |
|-------|--------|-------|
| 1. Inline call context | Low | Append "Calls: foo, bar" to embedding text |
| 2. Same-file call edges | Medium | No import resolution needed |
| 3. Module-level imports | Medium | File→File edges, simpler than function calls |
| 4. Cross-file function calls | High | Full graph capability |
| 5. Graph query interface | High | "What calls X?" as distinct retrieval mode |

**Recommendation:** Phases 1-3 give architectural value before full function-level graph.

---

### 3. Docstring Generation Module

| Verdict | **Yes - essential for arbitrary codebases** |
|---------|---------------------------------------------|

**For generic tool, this is high priority:**
- Can't assume documentation in third-party code
- Generated docstrings improve semantic search even if imperfect
- Enables better answers for undocumented functions

**Critical missing piece:** Existing docstring extraction is broken (`docstring` field always None).

**Implementation approach:**
1. **Fix extraction first** - parse `///`, `//!` (Rust), `"""..."""` (Python)
2. **Generate for undocumented** - selective, not blanket
3. **Content-hash caching** - regenerate only on code changes
4. **Tiered models** - Haiku for bulk, better model for complex/central functions
5. **Persist separately** - survives re-ingestion

**Critical: Don't overwrite user comments**
- User-written docstrings (even AI-assisted via Copilot) must be preserved
- Store generated docstrings in separate DB/file, never modify source
- Detection strategy:
  - If source has docstring → use it, mark as `source: user`
  - If source lacks docstring → generate, mark as `source: generated`
  - On re-ingestion: if source now has docstring, prefer it over our generated one

**CI/CD integration potential:**
- GitHub Action: on push/PR, run incremental ingestion + docstring generation
- Could output PR comment with "X functions now documented"
- Requires infrastructure: ingestion API, generation pipeline, persistent storage
- Future: auto-PR to add generated docstrings to source (opt-in, reviewable)

---

### 4. Auto-Generated Repo Summaries
**Problem:** "Tell me about this codebase" retrieves scattered chunks.

**Solution:**
- Infer purpose from README, package manifests, structure
- Extract tech stack from dependencies
- Identify entry points and main modules
- LLM-summarize repos without good READMEs

**Priority:** High - critical for multi-repo and unfamiliar codebases.

---

### 5. Hybrid Search (Keyword + Semantic)
**Problem:** "Show me UserService" may return semantically similar but wrong results.

**Solution:**
- Combine BM25 (lexical) with vector similarity
- Boost exact identifier matches
- LanceDB supports both

**Priority:** High - precision matters for code search.

---

### 6. Intent Classification / Query Routing
**Problem:** All queries use same retrieval strategy (5 code, 2 readme, etc.)

**Solution:**
- Classify query type before retrieval:
  - `overview` → README, summaries, CrateChunks
  - `implementation` → CodeChunks
  - `relationship` → call graph (when available)
  - `comparison` → cross-project retrieval
- Adjust limits dynamically

**Priority:** Medium-High - improves relevance significantly.

---

### 7. Multi-Repo Query Support
**Problem:** Portfolio is already multi-project; tool should generalize.

**Solution:**
- Explicit repo/project scoping in queries
- Cross-repo comparison queries ("how does A handle errors vs B?")
- Unified search with project attribution
- Project-aware context building

**Priority:** Medium - already partially exists via `project_name`.

---

### 8. Function Signature Extraction + Type Generation
**Problem:** Type information buried in raw code, wastes embedding dimensions. Python often lacks type hints.

**Solution:**
- Extract: function name, parameters, return type, type hints
- Store as structured metadata
- Include in embedding text: `fn process(input: &str) -> Result<Output>`
- Enables type-aware queries

**Auto-generate missing Python types (like docstrings):**
- LLM-infer types for untyped Python functions
- Same pattern as docstring generation:
  - Don't overwrite user-provided types
  - Store separately with `source: generated` vs `source: user`
  - Content-hash caching, regenerate on code change
- CI/CD integration: GitHub Action to generate types on push/PR
- Future: auto-PR to add type stubs (opt-in, reviewable)

**Priority:** Medium - improves precision for API-related questions.

---

### 10. Code Topology Construction (RAPTOR for Code)
**Idea:** Recursively cluster functions into emergent architectural understanding.

**How RAPTOR works (for text):**
1. Embed chunks → cluster by similarity → summarize each cluster
2. Embed summaries → cluster again → summarize
3. Recurse until single root summary
4. Result: tree of abstractions for multi-level retrieval

**Applied to code:**
1. Start with function embeddings
2. Cluster semantically similar functions
3. LLM-summarize: "These N functions handle authentication..."
4. Embed cluster summaries, repeat
5. Result: emergent architectural tree

**Key insight: Bottom-up vs Top-down architecture**

| Approach | Source | Reveals |
|----------|--------|---------|
| **Top-down** (folder/file) | Directory structure | What author *intended* |
| **Bottom-up** (clustering) | Code semantics | What code *actually does* |

Both are valuable. Discrepancies reveal architectural drift.

**Realism assessment:**

| Aspect | Assessment |
|--------|------------|
| Technical feasibility | **High** - embeddings, clustering, LLM summarization all exist |
| Implementation complexity | **Medium-High** - cluster quality tuning, recursion depth, cost management |
| Novel value | **High** - few tools do emergent architecture discovery |

**Challenges:**
- Code semantics differ from text - similar code may serve different purposes
- Cross-cutting concerns (logging, errors) cluster together but aren't a "module"
- Cluster quality depends heavily on embedding quality
- Cost: LLM summarization at each level
- Evaluation: how to validate emergent structure is meaningful?

**Likely implementation approach:**

```
Phase 1: Static clustering
├── Embed all functions
├── Cluster with HDBSCAN (handles varying density, noise)
├── LLM-summarize each cluster
└── Store as ClusterSummaryChunk (level 0)

Phase 2: Recursive abstraction
├── Embed level-0 summaries
├── Cluster again
├── Summarize
└── Repeat until convergence or max depth

Phase 3: Integration
├── Query routing: architecture queries → cluster summaries
├── Drill-down: cluster → member functions
└── Comparison: emergent structure vs folder structure
```

**Integration with portfolio-rag-chat:**
- New chunk type: `ClusterSummaryChunk { level, member_ids, summary, centroid }`
- Complements FolderChunk (top-down) with bottom-up view
- Architecture queries retrieve from both, compare/synthesize
- "What's the architecture?" → top-down + bottom-up perspectives

**Research questions to explore:**
- Best clustering algorithm for code (HDBSCAN? hierarchical? spectral?)
- Optimal cluster size / recursion depth
- How to handle cross-cutting concerns (exclude? separate cluster type?)
- How to evaluate quality of emergent structure?

**Priority:** Medium-High (research-focused, high differentiation value)

---

### 11. Incremental Ingestion
**Problem:** Re-ingestion processes everything, no delta updates.

**Solution:**
- Content-hash each chunk
- Skip unchanged files/functions on re-ingest
- Delete removed chunks
- Essential for large codebases

**Priority:** Medium - becomes critical at scale.

---

## Revised Priority Matrix

| Priority | Improvement | Effort | Impact | Notes |
|----------|-------------|--------|--------|-------|
| **1** | Fix docstring extraction | Low | High | Currently broken |
| **2** | Inline call context | Low | Medium | Quick win for relationships |
| **3** | Intent classification | Medium | **High** | Prerequisite for hierarchy |
| **4** | Folder/File-level embeddings | Medium | **High** | Architecture understanding |
| **5** | Auto-generate repo summaries | Medium | High | Critical for unfamiliar code |
| **6** | Hybrid search | Medium | High | Precision improvement |
| **7** | Docstring generation pipeline | Medium | High | For undocumented code |
| **8** | Function signature + type generation | Medium | Medium | Type-aware search, Python type inference |
| **9** | Same-file call edges | Medium | Medium | Relationship queries |
| **10** | Large function chunking | Medium | Medium | Handle oversized functions |
| **11** | Incremental ingestion | Medium | Medium | Scale efficiency |
| **12** | Cross-file call graph | High | Medium | Full relationship tracking |
| **13** | Graph query interface | High | Medium | "What calls X?" queries |
| **14** | Code topology (RAPTOR) | High | **High** | Emergent architecture, high differentiation |

---

## Differentiation from Commercial AI Agents

### What Cursor/Copilot/Windsurf do well:
- Code generation and completion
- In-editor assistance, quick fixes
- Chat about current file/selection
- Refactoring with context

### What they DON'T do (our opportunity):

| Gap | Why it's hard for them | Our advantage |
|-----|------------------------|---------------|
| **Multi-repo understanding** | Optimized for single project | Portfolio/org-wide view by design |
| **Persistent enrichment** | Session-based, no accumulation | Generated docs/types persist across sessions |
| **Architecture-first queries** | File/function level focus | Folder/module/system level embeddings |
| **Relationship queries** | No call graph persistence | "What calls X?", "Show the flow" |
| **Cross-project comparison** | Can't compare codebases | "How does A handle errors vs B?" |
| **Understanding others' code** | Assumes you're the author | Optimized for unfamiliar codebases |
| **Custom ingestion** | Fixed indexing strategy | Control what/how things are indexed |

### Our specialized niche: **Code Understanding, not Code Generation**

| Commercial tools | This tool |
|------------------|-----------|
| "Write me a function that..." | "How does this function work?" |
| "Fix this bug" | "How could this error propagate?" |
| "Refactor this" | "What's the architecture here?" |
| "Add tests" | "What calls this? What might break?" |

### Key differentiators to build:

1. **Multi-repo as first-class** - not an afterthought
   - Cross-repo search, comparison, pattern detection
   - "Show me all auth implementations across repos"

2. **Persistent knowledge accumulation**
   - Generated docstrings, types, summaries don't disappear
   - Gets smarter over time, not just per-session

3. **Architecture-level understanding**
   - Hierarchical embeddings (folder → file → function)
   - Call graphs for flow/dependency questions
   - Module boundaries and coupling analysis

4. **Debugging/investigation focus**
   - "How does error reach here?"
   - "What's the call chain to this function?"
   - "What depends on this module?"

5. **Onboarding/explanation use case**
   - New team member understanding codebase
   - Code review assistance (understand unfamiliar code)
   - Due diligence on third-party dependencies

6. **Emergent architecture discovery (RAPTOR for code)**
   - Bottom-up clustering reveals what code *actually does*
   - Compare with folder structure (what author *intended*)
   - Detect architectural drift, hidden coupling
   - No commercial tool does this

### Anti-goals (don't compete here):
- Real-time code completion (they're better, tighter IDE integration)
- Inline quick fixes (requires deep IDE integration)
- Generation of new code (not our focus)

---

## Project Architecture

The system is a **Cargo workspace** with three crates in one repository:

```
portfolio-rag-chat/
├── Cargo.toml                    # workspace root
├── crates/
│   ├── coderag-types/            # shared types
│   └── code-raptor/              # ingestion CLI
└── src/                          # root crate (query interface)
```

### coderag-types (Shared Crate)
**Focus:** Type definitions shared between producer and consumer

- `CodeChunk`, `FolderChunk`, `FileChunk`, `ReadmeChunk`
- `CrateChunk`, `ModuleDocChunk`, `ClusterChunk`
- Serde serialization for LanceDB storage

### code-raptor (Sub-Crate)
**Focus:** Build rich, queryable knowledge graphs from code

| Component | What it does |
|-----------|--------------|
| Ingestion pipeline | Parse code, extract chunks |
| Hierarchical embeddings | Folder → file → function vectors |
| Call graph extraction | Relationship edges |
| Docstring/type generation | Enrich undocumented code |
| RAPTOR clustering | Emergent architecture discovery |
| Incremental updates | Efficient re-indexing |

**Output:** Code knowledge graph (vectors + edges + metadata in LanceDB)
**Runs:** On code changes (CI/CD, manual trigger)
**Optimizes for:** Coverage, richness, accuracy
**Named after:** RAPTOR technique (Recursive Abstractive Processing for Tree-Organized Retrieval)

### portfolio-rag-chat (Root Crate)
**Focus:** Query interface consuming knowledge graphs

| Component | What it does |
|-----------|--------------|
| Intent classification | Detect query type |
| Query routing | Select retrieval strategy |
| Hybrid search | BM25 + semantic |
| Multi-repo scoping | Filter/compare across repos |
| Context building | Format for LLM |
| Conversation handling | Follow-ups, drill-down |

**Output:** Answers to user questions
**Runs:** On user queries (real-time)
**Optimizes for:** Relevance, latency, coherence

### Why workspace with sub-crates?

| Reason | Benefit |
|--------|---------|
| **Single repo** | One git history, simpler maintenance |
| **Shared Cargo.lock** | Consistent dependency versions |
| **Path dependencies** | Instant iteration, no publish step |
| **Different update frequencies** | code-raptor: on code change. root crate: on query |
| **Different optimization goals** | code-raptor: coverage. root crate: relevance |
| **Reusability** | code-raptor can be published to crates.io later |
| **Testability** | Evaluate indexer quality separately from chatbot quality |

### Mapping improvements to crates:

| Improvement | Crate |
|-------------|-------|
| Hierarchical embeddings | code-raptor |
| Call graph extraction | code-raptor |
| Docstring generation | code-raptor |
| Type generation | code-raptor |
| RAPTOR clustering | code-raptor |
| Incremental ingestion | code-raptor |
| Repo summaries | code-raptor |
| --- | --- |
| Intent classification | root crate |
| Query routing | root crate |
| Hybrid search | root crate |
| Multi-repo queries | root crate |
| Graph query interface | root crate |

### Dependency Direction
```
coderag-types (shared)
    ↑ depends on
code-raptor (producer)        portfolio-rag-chat (consumer)
    ↓ writes to                    ↓ reads from
         [LanceDB]
```

**No circular dependencies** - both crates depend on coderag-types, not each other.
They communicate via LanceDB schema.

---

## Key Insights

### Core principles for a generic code Q&A tool:
- **Can't assume documentation** → extraction + generation are essential
- **Can't assume familiarity** → architecture/hierarchy queries come first
- **Can't assume code quality** → type inference, auto-summaries fill gaps
- **Must handle arbitrary structure** → auto-detection over configuration
- **Relationships matter** → call graphs serve implementation, architecture, AND debugging
- **Precision matters** → hybrid search (keyword + semantic) for exact matches
- **Intent varies** → query routing to appropriate granularity/retrieval strategy
