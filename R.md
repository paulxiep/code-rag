# Track R — Code Raptor: Emergent Code Topology

Detailed design for Track R. The roadmap-level summary lives in
[development_plan.md](development_plan.md); this document is the implementation
plan.

**Goal.** Let **Code Raptor** discover the *real* modules a codebase forms —
bottom-up — by building an explicit relation topology over the code and
partitioning it, then compare that emergent structure to the folder layout. The
same topology powers structural analytics (which functions to read first, hidden
cross-module coupling, dependency cycles) and an interactive architecture view.
Realizes Vision #14 (Code Topology / RAPTOR) and extends Track C (the existing
`calls` graph).

"Graph" here is just the shape of the topology — the *identity* is Code Raptor:
a deterministic topology engine that turns persisted relation edges into
architectural insight.

---

## 1. What changed vs the original Track R

The original Track R sketch clustered on Track-D-generated embedding summaries
(HDBSCAN / hierarchical / spectral). This revision builds an explicit **relation
topology** from typed edges and derives structure by **community detection** on
it.

| Original | Revised |
|---|---|
| Cluster on embedding summaries | Partition a **relation topology** (calls/imports/contains/implements/extends/…) by community detection |
| Clusters reflect semantic similarity | Clusters reflect **code structure** (who depends on whom) — deterministic |
| Hard dependency on Track D | **Removed** — template summaries need no LLM; LLM summaries optional |
| Clustering only | Clustering **+ structural analytics + dependency cycles + interactive view** from one topology |

A second shift this revision makes is **structural**: the marquee `code-raptor`
name moves to the topology engine, and the parser/ingester it described before
becomes `code-rag-ingest`. See §3.

---

## 2. Principles

- **Pure / native split.** The topology *model* + light traversal + degree
  analytics live in `code-rag-engine` (pure, compiles to `wasm32` — the GH-Pages
  demo depends on this). **Community detection, betweenness and cycle detection
  run native, at ingestion time, in the `code-raptor` topology crate**; the
  browser consumes **precomputed artifacts** (community assignments, analytics, a
  viz JSON) and never runs the heavy algorithms.
- **Reuse, don't replace.** The existing wasm-safe
  `code-rag-engine::graph::CallGraph` (BFS callers/callees, `find_path`, the C2
  `reserve_graph_slots` result-protection the harness relies on) is kept
  untouched. A pure `RelationGraph` is added *beside* it for the new relations.
- **Declarative / additive.** A new chunk type and a new edge table light up in
  routing automatically — the same way A2/A4 folder/file chunks did (an empty
  table is a harmless no-op arm).
- **Deterministic first.** Template cluster summaries, degree centrality,
  cohesion scores and dependency cycles are deterministic and reproducible across
  runs. LLM summaries and recursive abstraction are optional layers on top.

---

## 3. Crate layout

Track R introduces one crate move and reclaims the marquee name for the feature
it belongs to. Today `code-raptor` is the *ingester* (tree-sitter parsing,
call-edge resolution, chunk + edge export) — the heavy topology algorithms have
no natural home. We split that out:

```
crates/
  code-raptor/   (parser/ingester today)   ──►  code-rag-ingest/
      tree-sitter parsing, call/edge             same responsibilities, renamed.
      resolution, reconcile, chunk +             SoC: source → chunks + raw relation edges.
      edge export

  (new)                                     ──►  code-raptor/   ← reclaims the marquee name
                                                  THE topology engine: RelationGraph build
                                                  from persisted edges, Leiden/Louvain
                                                  community detection, betweenness +
                                                  cohesion + dependency cycles, ClusterChunk
                                                  summaries, architecture report + viz/exports.
```

Why split:

- **SoC.** `code-rag-ingest` has one job (parse → persist chunks + raw relation
  edges). `code-raptor` has one job (derive topology from those edges). Matches
  the project's Declarative / Modular / SoC principles.
- **Dependency direction.** `code-raptor` (topology) depends on `code-rag-store`
  (to read `graph_edges` / `call_edges`) and `code-rag-types`; it does **not**
  depend on `code-rag-ingest`. Ingestion *writes* edges; topology *reads* them.
  Both are invoked by the ingestion-time orchestrator and by `code-rag-mcp`.
- **Brand.** The user-facing `code-raptor` name is preserved — it now names the
  topology brain, which is what "RAPTOR" is about. (`raptor` is the shorter
  alternative if a non-`code-rag-*` crate name is ever preferred.)

| Crate | Role in Track R | wasm? |
|---|---|---|
| `code-rag-types` | `GraphEdge`, `EdgeRelation`, `EdgeContext`, `EdgeConfidence`, `ClusterChunk`, `ExportGraphEdge` | yes (serde only) |
| `code-rag-ingest` | richer-edge **extraction/resolution** (parsing side); `extract_type_relations`, comment-scan, `edge_resolution` | no |
| `code-raptor` | RelationGraph build, community detection, betweenness, cohesion, dependency cycles, ClusterChunk summaries, architecture report, viz + exports | no (native) |
| `code-rag-engine` | pure `RelationGraph` + degree centrality + relation cues; reuses `CallGraph` | **yes** |
| `code-rag-store` | `graph_edges` + `cluster_chunks` LanceDB tables; community-id persistence | no |
| `code-rag-ui` | interactive topology view (consumes precomputed artifacts) | yes |

**Rust algorithm choices (native, in `code-raptor`):** `petgraph` for the
in-memory topology; a native deterministic implementation of **Louvain** for
community detection (**Leiden is deferred** — Rust has no graspologic/Leiden
equivalent and `petgraph` ships no community detection, so this is a
from-scratch implementation either way; see §4 R2); Brandes' algorithm for
edge-betweenness (node cap to bound the O(V·E) cost); Johnson's algorithm for
elementary-cycle (circular-dependency) detection over the import/contains
subgraph. None of these enter `code-rag-engine` or `code-rag-ui`, so the wasm
build stays clean.

---

## 4. Milestones

Order for impact: **R0 → R1 → R2 → R3** (structure + retrieval-quality wins)
**→ R4 → R5** (features / insight). R0 (crate split) and R1 are the only hard
prerequisites; R5 needs Track A.

| Milestone | Effort (days) | Eng-months | Notes |
|---|---|---|---|
| R0 Crate split | 2–3 | ~0.1 | rename parser → `code-rag-ingest`; scaffold `code-raptor` topology crate; rewire orchestrator + MCP |
| R1 RelationGraph + richer edges | 6–8 | ~0.3 | structural relations + taxonomy expansion (`Embeds`, `ReExports`, `References` context tags); `RationaleFor` optional |
| R2 Community detection + cohesion | 4–5 | ~0.2 | deterministic Louvain (Leiden deferred) + hub exclusion + per-community cohesion scoring |
| R3 ClusterChunk | 3–4 | ~0.15 | template summaries → `cluster_chunks` table → Overview/architecture arm; optional LLM tier |
| R4 Analytics + report | 3–4 | ~0.15 | centrality, betweenness, **dependency cycles (Johnson)**, **surprising-connection ranking**, suggested questions |
| R5 Visualization + comparison + exports | 4–6 | ~0.25 | interactive view + Mermaid + **GraphML** (Obsidian optional); architecture drift vs Track A |
| **Total** | **~22–30** | **~1.0–1.3** | up from ~2.5–3 weeks due to the crate split + added analytics/exports |

Effort is a neutral sizing estimate, not a schedule (~22 working days ≈ 1
engineering-month).

### R0 — Crate split (prep)

Reshape the workspace so the topology work has a home and the marquee name lands
on it.

- Rename the current `code-raptor` crate directory + `[package] name` to
  `code-rag-ingest`; keep its module layout (`ingestion/`, `edge_resolution.rs`,
  `export.rs`, `orchestrate.rs`) intact.
- Scaffold a new `code-raptor` crate (native) for the topology engine; add it as
  a workspace member; depend on `code-rag-types` + `code-rag-store` (+ `petgraph`).
- Update every `use code_raptor::` reference — notably in `code-rag-mcp` and the
  ingestion-time orchestrator — to `code_rag_ingest::` for parsing entrypoints,
  and route post-ingest clustering/analytics calls to the new `code-raptor`.
- The ingest subcommands move with the parser to `code-rag-ingest` (e.g.
  `code-rag-ingest ingest` / `code-rag-ingest export`); the `code-raptor` name is
  reclaimed by the topology engine. MCP `ingest` *behavior* stays identical.
  Expose a topology stage (cluster/analytics) that can run after ingest or be
  re-run alone (cluster-only) without re-parsing.
- **Testable.** `cargo build --workspace` and `trunk build --features standalone`
  stay green; the existing ingest CLI + MCP `ingest` behave identically.

### R1 — RelationGraph + richer relationship edges

Today only `calls` edges are persisted (`CallEdge`, 3-tier resolution). R1 adds
the other structural relations — and a richer taxonomy — so both relationship
retrieval and clustering have a real topology to work on.

- **Types (`code-rag-types`).** `GraphEdge { edge_id, source_chunk_id,
  target_chunk_id, source_identifier, target_identifier, source_file,
  target_file, project_name, relation, context, confidence }`.
  - `EdgeRelation { Calls, Imports, Contains, Implements, Extends, References,
    Embeds, ReExports, RationaleFor }` — `Embeds` for struct/record composition;
    `ReExports` for re-exported symbols; `RationaleFor` for inline-comment
    rationale (see below).
  - `EdgeContext { ParameterType, ReturnType, GenericArg, FieldType, Attribute,
    None }` — an optional tag carried on `References` edges so a type reference
    records *where* it occurs.
  - `EdgeConfidence { Extracted, Inferred, Ambiguous }`. Deterministic,
    relation-aware `edge_id`.
  - `calls` edges are **not** duplicated here — they are projected from the
    existing `call_edges` table at topology-build time, so C1/C2's call-specific
    `resolution_tier` semantics stay intact.
- **Extraction (`code-rag-ingest`).**
  - `imports` / `re_exports` — `imports` is already extracted
    (`LanguageHandler::extract_file_imports`, surfaced today in
    `FileChunk.imports`); promote the resolved import to an `Imports` edge, and
    flag re-exports (`pub use`, `export … from`) as `ReExports`. Near-zero new
    parsing.
  - `contains` — derived from the chunk hierarchy (folder ⊇ file ⊇ definition);
    no parsing, same data `FolderChunk`/`FileChunk` already compute.
  - `implements` / `extends` / `embeds` — new tree-sitter work via a new
    `extract_type_relations` method on `LanguageHandler` (default empty,
    mirroring how `extract_calls` / `extract_file_imports` were added). Rust
    `impl Trait for T` + trait bounds + struct field composition (`Embeds`);
    Python base-class lists; TypeScript `implements` / `extends`. Skip Go
    structural interface satisfaction (same spirit as C1's trait-dispatch skip).
  - `references` (with `EdgeContext`) — type identifiers appearing in parameter,
    return, generic-argument, field or attribute position, tagged accordingly.
  - **`RationaleFor` *(optional / deferrable — ship structural relations first)*.**
    A comment-scan pass harvests inline `NOTE:` / `WHY:` / `HACK:` rationale
    adjacent to a definition and links it to that definition. Novel and
    high-signal for "why does X exist?" queries, but it adds a comment-scanning
    pass for a modest recall payoff — so it is the *last* thing in R1 and may slip
    to a follow-up without blocking the topology.
  - Resolve targets with the existing identifier index + per-file imports
    (reuse `edge_resolution.rs`); tag `Extracted` for AST-direct, `Inferred` for
    heuristic/unique-global, `Ambiguous` otherwise.
- **Storage (`code-rag-store`).** New `graph_edges` scalar table (no vectors),
  upsert/query mirroring `call_edges`; `relation` / `context` / `confidence`
  stored as their string tags.
- **Engine (`code-rag-engine`).** A pure `RelationGraph` (adjacency keyed by
  relation) built from `GraphEdge`s; teach `detect_direction` /
  `extract_target_term` the new cues ("what implements X?", "what extends Y?",
  "what imports Z?", "what embeds W?"). Relationship-intent augmentation consults
  it alongside the `CallGraph`.
- **Testable.** Relationship-intent queries beyond `calls` ("what implements
  `Embedder`?") resolve in the harness; existing relationship recall does not
  regress.

### R2 — Community detection + cohesion (emergent modules)

- **Node set & edge selection (resolved).** Partition is run over **one
  undirected graph** whose nodes are the chunk ids appearing on the kept edges.
  Kept edges: `calls ∪ imports ∪ implements/extends/embeds/references ∪
  **file→function `contains`**`, all at equal weight. **Folder→file `contains`
  is excluded from the partition input** — high-level folders are frequently not
  cohesive, so feeding the folder tree into clustering would make communities
  recover the folders and make the R5 emergent-vs-folder comparison
  self-fulfilling. File-level containment is kept (functions in one file are
  usually genuinely cohesive). Folder edges remain stored for retrieval and the
  R5 comparison; they are simply not partition inputs.
- **Algorithm (resolved): deterministic Louvain; Leiden deferred.** Run a native
  **Louvain** (modularity maximization, seeded RNG, lexical tie-break) →
  communities ordered by size with a min-chunk-id tie-break. **Leiden is
  deferred** to an optional later refinement pass; its only added guarantee
  (well-connected communities) is needed only if a spot-check shows
  internally-disconnected Louvain communities. Revisit trigger: such a community
  appears in the R4 report. The deferral is flagged in code at the partition
  entry point.
- **Cross-cutting handling:** exclude very-high-degree utility nodes (logging,
  error handling) — and, by construction, high-fan-out file hubs — from
  partitioning, then reattach by majority vote; split oversized communities
  (> ~25% of the topology). Degree-based exclusion is what neutralizes
  non-cohesive containment hubs (high-level folder/large-file nodes are
  high-degree → excluded), which is why file-level `contains` can be kept
  safely.
- **Cohesion score.** Compute a per-community cohesion score (actual
  intra-community edges / max possible) and persist it with each community. Use
  it to drive low-cohesion re-splitting (re-split large, ≥ ~50-node, < ~0.05
  communities) and to surface community quality in the R4 report.
- Native, ingestion-time (heavy → not wasm); **per-project** scope
  (`build_topology(project_name = Some)`), a corpus-wide union pass left as an
  optional later add-on. Determinism: sort nodes/edges by id before partitioning
  + seeded RNG + community re-index by `(size desc, min member chunk_id)`, so
  identical input → identical communities and ids across runs.
- Persist a community id (+ cohesion) per chunk in an **additive side table**
  (`community_assignments`), avoiding any migration of the `code_chunks` schema.
- **Testable.** Communities + ids + cohesion are stable across runs; coherence
  spot-check vs folder structure (cluster purity).

### R3 — ClusterChunk (summaries + retrieval)

- New `ClusterChunk { cluster_id, project_name, member_chunk_ids,
  key_identifiers, files, summary_text, chunk_id, content_hash,
  embedding_model_version }`. Produced by the `code-raptor` topology crate.
- **Template summary tier** (deterministic, ships first), mirroring the
  `FolderChunk` template:
  `Cluster N (K functions across M files): key types …; key functions …; spans
  …; dominant relation …; cohesion …; likely concern: <most-central member>.`
- Embed `summary_text` (BGE-small, same path as folder/file chunks); new
  `cluster_chunks` table; add to the collapsed-tree fan-out for
  **Overview/architecture** intent (weak arm elsewhere); RRF-fused like every
  chunk type.
- **Optional LLM tier** (Track D upgrade): "These N functions handle
  authentication…". Not a blocker.
- **Testable.** Add ~5 architecture/overview queries; cluster chunks hit for
  "what are the main subsystems / what handles X"; Overview recall ≥ baseline.

> **Result — clusters gated OFF on all intents (measured 2026-06-10).** R3 shipped
> the full slice (type, template, `cluster_chunks` table, retrieval arm, export,
> WASM), but the **ground-truth per-intent sweep falsified the "Overview recall
> improves" hypothesis**: as a retrieval arm, cluster summaries *displace* the
> code/folder chunks that already answer architecture queries — Overview recall@5
> −4pp at every limit, Comparison recall@10 −4pp at limit ≥2, Implementation
> recall@pool +3pp at limit 4 (the only positive, with a recall@10 cost). So the arm
> is gated off (`cluster_limit=0`, `cluster_vec=false`) per the project's
> empirical-gating standard, with all machinery left wired. The impl-pool signal
> suggests relevant clusters *are* retrieved but the cross-encoder buries them, so the
> revisit is **slot-protection (cf. C2)** or the **LLM cluster-summary tier** — not the
> current bare arm. (A secondary blocker: most architecture queries classify as
> implementation/relationship, not overview — a B4-classifier concern.) See
> [development_log.md](development_log.md) 2026-06-10.

### R4 — Structural analytics + architecture report

- **Central nodes** (degree centrality — "the functions to understand first";
  degree is wasm-cheap so the demo can compute it).
- **Cross-module bridges** (edge-betweenness across communities — hidden
  coupling; native, precomputed, node-capped).
- **Surprising-connection ranking.** Rank cross-community bridges by an
  *unexpectedness* score (bridge weight × low community-overlap), not raw
  betweenness alone, so the report leads with genuinely surprising coupling.
- **Dependency cycles.** Johnson's-algorithm elementary-cycle detection over the
  import/contains subgraph surfaces circular dependencies (deduplicated by
  rotation). Deterministic and native; high portfolio value.
- **Cohesion** scores (from R2) reported per community.
- **Suggested questions** the topology is positioned to answer.
- Emit a markdown **architecture report** artifact at ingest (central nodes,
  bridges, surprising connections, dependency cycles, communities + cohesion,
  suggested questions). Optional: inject top central nodes as guaranteed Overview
  context (analogous to C2 slot reservation).
- **Testable.** Report generates; central-node list matches intuition on a known
  repo (e.g. `retrieve`, `ingest`); detected cycles match known circular deps;
  optional overview-injection doesn't regress recall.

> **Result — shipped 2026-08-06.** All analytics + the report landed
> (`data/reports/architecture_<project>.md`, `--report-dir` on the topology
> CLI). Two deviations: cycles use Tarjan SCC + bounded canonical DFS instead
> of Johnson's blocked search (blocking is unsound under a cycle-length cap),
> and the never-used `petgraph` dependency was removed (§3) — all graph
> algorithms are hand-rolled for determinism. The optional Overview
> central-node injection was skipped pending its own measured experiment (R3
> precedent). The first real report exposed an **R1 cross-project resolution
> leak** (ubiquitous identifiers like `String` resolving into other projects);
> report-level project filter shipped, resolution-level fix tracked as a
> follow-up. See [development_log.md](development_log.md) 2026-08-06.

### R5 — Architecture comparison + visualization + exports

- **Architecture comparison** (requires Track A): compare emergent communities
  (bottom-up) against folder/file hierarchy (top-down); surface divergence
  (architectural drift); route "What's the architecture?" to both views.
- **Interactive topology view** in the Leptos / GH-Pages demo: precompute
  `graph_viz.json` (nodes community-colored + degree-sized, edges by
  relation/confidence; node cap ~5000); force layout; legend; **click a node →
  run a code-rag query**. Optional Mermaid call-flow export pairing with the
  existing `find_path`.
- **Export formats.**
  - **GraphML** (primary added format) — load the topology in Gephi / yEd for
    offline exploration; cheap to emit, high analytic value.
  - **Obsidian / wiki export** *(optional)* — one note per community + per
    central node, with backlinks. Lower-value for a browser-demo portfolio than
    GraphML, so it is optional and slots in only if a vault/wiki surface is
    wanted.
- **Testable.** Demo renders + click-to-query works; comparison highlights at
  least one real top-down/bottom-up divergence; GraphML loads in a graph tool.

---

## 5. Research vs production

Community detection (R2) + cohesion scoring + template ClusterChunks (R3) +
structural analytics (R4: centrality, betweenness, surprising connections,
dependency cycles) are deterministic enough to ship. R2 ships **Louvain only**;
**Leiden is deferred** as an optional refinement (see §4 R2). **Recursive
abstraction** (cluster the cluster summaries into a multi-level tree) stays
time-boxed research, attempted only if R2/R3 land and a multi-level view
demonstrably helps.
Architecture comparison (R5) is exploratory (depends on Track A + human judgement
of "drift").

---

## 6. Verification

- `cargo build --workspace` proves the R0 crate split is clean (parser →
  `code-rag-ingest`, topology → `code-raptor`); **and** `trunk build --features
  standalone` (`code-rag-ui`) stays green → confirms no native topology
  dependency leaked into `code-rag-engine` / `code-rag-ui`.
- Topology re-run (cluster-only) works without re-parsing.
- Re-ingest the corpus, run the harness (classifier + ground-truth), compare
  per-intent recall@5 vs the V3.3 baseline (R1 → relationship, R3 → overview);
  dataset-freeze policy (add tagged cases, don't modify existing).
- New analytics appear in the architecture report (dependency cycles, cohesion,
  surprising connections) and GraphML export loads in a graph tool.
- Exercise new topology MCP tools (community / central-nodes / cycles / path) and
  the demo topology view.

---

## 7. Crate mapping

| Work | Crate |
|---|---|
| `GraphEdge`, `ClusterChunk`, edge/relation/context/confidence enums | code-rag-types |
| Richer-edge **extraction/resolution** (`extract_type_relations`, comment-scan, `edge_resolution`) | code-rag-ingest |
| RelationGraph build, community detection, cohesion, betweenness, dependency cycles, ClusterChunk summaries, report + viz + exports | code-raptor |
| `RelationGraph`, degree analytics, relation cues, routing | code-rag-engine |
| `graph_edges` + `cluster_chunks` tables, community-id persistence | code-rag-store |
| Interactive topology view, cluster / architecture surfaces | code-rag-ui |

---

## 8. References

- RAPTOR (ICLR 2024) — collapsed-tree multi-level retrieval.
- Leiden — Traag, Waltman & van Eck (2019), *From Louvain to Leiden:
  guaranteeing well-connected communities*.
- Louvain — Blondel, Guillaume, Lambiotte & Lefebvre (2008), *Fast unfolding of
  communities in large networks*.
- Edge-betweenness community structure — Girvan & Newman (2002).
- Elementary circuits — Johnson (1975), *Finding all the elementary circuits of a
  directed graph*.
