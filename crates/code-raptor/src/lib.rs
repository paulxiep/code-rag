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
//! R0 scaffold: the real algorithms arrive in R2+. For now this exposes the
//! topology-stage seam so the ingestion-time orchestrator and `code-rag-mcp`
//! have a stable call site, and so a cluster-only re-run can be wired without
//! re-parsing.

mod cluster;
mod clusterchunk;
mod louvain;
mod topology;

use std::collections::HashMap;

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
}

/// Options for a topology build/refresh over an already-ingested index.
#[derive(Debug, Clone)]
pub struct TopologyOpts {
    /// Path to the LanceDB index produced by `code-rag-ingest`.
    pub db_path: String,
    /// Restrict to a single project, or `None` for the whole corpus.
    pub project_name: Option<String>,
}

/// Build (or refresh) the topology for an already-ingested index.
///
/// R2: reads the persisted edge tables, builds the relation topology, runs
/// deterministic community detection (Louvain; Leiden deferred), and persists a
/// community id + cohesion per code chunk to `community_assignments`. Runs
/// per-project (the unit emergent modules are compared against folders within);
/// `project_name = None` refreshes every ingested project. R3+ will extend this
/// to also write `ClusterChunk`s and the architecture report.
///
/// Deliberately separate from ingestion so it can run right after an ingest or
/// be re-run cluster-only without re-parsing.
pub async fn build_topology(opts: TopologyOpts) -> Result<(), TopologyError> {
    let store = VectorStore::new(&opts.db_path, STORE_DIM).await?;
    // One embedder for the whole run (model load is the expensive part).
    let embedder = FastEmbedImpl::new()?;
    let projects = match &opts.project_name {
        Some(p) => vec![p.clone()],
        None => store.list_projects().await?,
    };
    for project in projects {
        build_for_project(&store, &embedder, &project).await?;
    }
    Ok(())
}

/// Detect communities + build ClusterChunks, and persist both, for one project.
async fn build_for_project(
    store: &VectorStore,
    embedder: &dyn Embedder,
    project: &str,
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

    info!(
        "topology: {count} chunks in {n_communities} communities, {} cluster chunks (project '{project}')",
        cluster_chunks.len()
    );
    Ok(())
}
