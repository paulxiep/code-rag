//! code-raptor: emergent code topology (Code Raptor).
//!
//! The topology engine. It reads the relation edges that `code-rag-ingest`
//! persists (`call_edges`, and—from R1—`graph_edges`) and turns them into
//! architectural insight: a `RelationGraph`, community detection, cohesion,
//! betweenness, dependency cycles, `ClusterChunk` summaries, and an
//! architecture report / viz exports.
//!
//! SoC: ingestion *writes* edges; topology *reads* them. This crate depends on
//! `code-rag-store` (to read the edge tables) and `code-rag-types`, never on
//! `code-rag-ingest`.
//!
//! The topology-stage seam (this file) is what the ingestion-time orchestrator
//! and `code-rag-mcp` call; a cluster-only re-run works without re-parsing.
//! R2 landed community detection + cohesion, R3 the `ClusterChunk` summaries,
//! R4 the structural analytics + architecture report. Viz/exports are R5.

mod analytics;
mod betweenness;
mod cluster;
mod clusterchunk;
mod cycles;
mod louvain;
mod report;
mod topology;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use code_rag_store::seams::Embedder;
use code_rag_store::{FastEmbedImpl, VectorStore};
use code_rag_types::{ClusterChunk, CodeChunk, CommunityAssignment};
use thiserror::Error;
use tracing::info;

use crate::topology::Topology;

/// Embedding dimension used when opening the store. The topology stage only
/// touches scalar tables (`call_edges`, `graph_edges`, `community_assignments`),
/// so this is never used to create a vector table — it just satisfies the
/// `VectorStore` constructor. Matches the project's BGE-small default.
const STORE_DIM: usize = 384;

/// Errors surfaced by the topology stage.
#[derive(Debug, Error)]
pub enum TopologyError {
    #[error("store error: {0}")]
    Store(#[from] code_rag_store::StoreError),
    #[error("embed error: {0}")]
    Embed(#[from] code_rag_store::EmbedError),
    #[error("report io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Options for a topology build/refresh over an already-ingested index.
#[derive(Debug, Clone)]
pub struct TopologyOpts {
    /// Path to the LanceDB index produced by `code-rag-ingest`.
    pub db_path: String,
    /// Restrict to a single project, or `None` for the whole corpus.
    pub project_name: Option<String>,
    /// Directory the architecture reports are written to (one
    /// `architecture_<project>.md` per project). `None` → `<db_path
    /// parent>/reports`, which with the default db path lands next to the
    /// harness report family in `data/reports/`.
    pub report_dir: Option<String>,
}

/// Build (or refresh) the topology for an already-ingested index.
///
/// Reads the persisted edge tables, builds the relation topology, runs
/// deterministic community detection (Louvain; Leiden deferred), persists a
/// community id + cohesion per code chunk to `community_assignments` plus one
/// `ClusterChunk` summary per community (R3), and emits the R4 architecture
/// report (centrality, bridges, surprising connections, dependency cycles).
/// Runs per-project (the unit emergent modules are compared against folders
/// within); `project_name = None` refreshes every ingested project.
///
/// Deliberately separate from ingestion so it can run right after an ingest or
/// be re-run cluster-only without re-parsing.
pub async fn build_topology(opts: TopologyOpts) -> Result<(), TopologyError> {
    let store = VectorStore::new(&opts.db_path, STORE_DIM).await?;
    // One embedder for the whole run (model load is the expensive part).
    let embedder = FastEmbedImpl::new()?;
    let report_dir: PathBuf = match &opts.report_dir {
        Some(d) => PathBuf::from(d),
        None => default_report_dir(&opts.db_path),
    };
    let projects = match &opts.project_name {
        Some(p) => vec![p.clone()],
        None => store.list_projects().await?,
    };
    for project in projects {
        build_for_project(&store, &embedder, &project, &report_dir).await?;
    }
    Ok(())
}

/// Default architecture-report directory for a db path (`<db parent>/reports`).
pub fn default_report_dir(db_path: &str) -> PathBuf {
    Path::new(db_path)
        .parent()
        .unwrap_or(Path::new("."))
        .join("reports")
}

/// Report path for one project inside the report dir; the project name is
/// sanitized so it is always a valid single filename component. Public so
/// project-removal tooling (`code-rag-ingest purge`) can delete the artifact
/// this crate emits.
pub fn report_path(report_dir: &Path, project: &str) -> PathBuf {
    let safe: String = project
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect();
    report_dir.join(format!("architecture_{safe}.md"))
}

/// Detect communities + build ClusterChunks, and persist both, for one project.
async fn build_for_project(
    store: &VectorStore,
    embedder: &dyn Embedder,
    project: &str,
    report_dir: &Path,
) -> Result<(), TopologyError> {
    let call_edges = store.get_all_edges(project).await?;
    let graph_edges = store.get_all_graph_edges(project).await?;

    let topo = Topology::build(&call_edges, &graph_edges);

    // Always clear stale rows first so a now-empty topology removes them.
    store
        .delete_community_assignments_by_project(project)
        .await?;
    store.delete_cluster_chunks_by_project(project).await?;
    if topo.is_empty() {
        // Remove a stale report too — same truthfulness rule as the row deletes.
        let stale = report_path(report_dir, project);
        if stale.exists() {
            std::fs::remove_file(&stale)?;
        }
        info!("topology: no edges for project '{project}' — skipped");
        return Ok(());
    }

    // R2: community assignments.
    let results = cluster::detect(&topo);
    let n_communities = results
        .iter()
        .map(|r| r.community_id)
        .max()
        .map(|m| m + 1)
        .unwrap_or(0);
    let assignments: Vec<CommunityAssignment> = results
        .iter()
        .map(|r| CommunityAssignment {
            project_name: project.to_string(),
            chunk_id: r.chunk_id.clone(),
            community_id: r.community_id,
            cohesion: r.cohesion,
        })
        .collect();
    let count = store.upsert_community_assignments(&assignments).await?;

    // R3: ClusterChunk summaries — fetch members, render, embed, upsert.
    let member_ids: Vec<String> = results.iter().map(|r| r.chunk_id.clone()).collect();
    let chunks = store.get_chunks_by_ids(&member_ids).await?;
    let members: HashMap<String, CodeChunk> = chunks
        .into_iter()
        .map(|c| (c.chunk_id.clone(), c))
        .collect();
    let cluster_chunks: Vec<ClusterChunk> = clusterchunk::build_cluster_chunks(
        project,
        &topo,
        &results,
        &members,
        &call_edges,
        &graph_edges,
    );
    if !cluster_chunks.is_empty() {
        let texts: Vec<&str> = cluster_chunks
            .iter()
            .map(|c| c.summary_text.as_str())
            .collect();
        let embeddings = embedder.embed_batch(&texts)?;
        store
            .upsert_cluster_chunks(&cluster_chunks, embeddings)
            .await?;
    }

    // R4: structural analytics + architecture report. Derived data — computed
    // fresh each run from what is already in scope, rendered pure, written as
    // one markdown artifact per project.
    let a = analytics::compute(project, &topo, &results, &call_edges, &graph_edges, &members);
    let lines = analytics::community_lines(project, &topo, &cluster_chunks, &members);
    let questions = report::suggested_questions(&a, &lines);
    let md = report::render_markdown(project, &a, &lines, &questions);
    std::fs::create_dir_all(report_dir)?;
    let path = report_path(report_dir, project);
    std::fs::write(&path, md)?;
    info!("topology: wrote architecture report {}", path.display());

    info!(
        "topology: {count} chunks in {n_communities} communities, {} cluster chunks (project '{project}')",
        cluster_chunks.len()
    );
    Ok(())
}
