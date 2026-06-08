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

use thiserror::Error;

/// Errors surfaced by the topology stage.
#[derive(Debug, Error)]
pub enum TopologyError {
    #[error("store error: {0}")]
    Store(#[from] code_rag_store::StoreError),
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
/// R0 stub: no-op. From R2 this reads the persisted edge tables, builds the
/// `RelationGraph`, runs community detection + analytics, and persists the
/// results (community ids, cluster chunks, architecture report). It is
/// deliberately separate from ingestion so it can run right after an ingest or
/// be re-run cluster-only without re-parsing.
pub async fn build_topology(_opts: TopologyOpts) -> Result<(), TopologyError> {
    // Intentionally empty until R2. Kept as a stable seam for callers.
    Ok(())
}
