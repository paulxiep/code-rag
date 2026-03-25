//! Load pre-computed chunk index from static JSON asset.

use gloo_net::http::Request;
use serde::Deserialize;

use code_rag_types::{CodeChunk, CrateChunk, ModuleDocChunk, ReadmeChunk};

/// A chunk paired with its pre-computed embedding vector.
#[derive(Debug, Clone, Deserialize)]
pub struct EmbeddedChunk<T> {
    #[serde(flatten)]
    pub chunk: T,
    pub embedding: Vec<f32>,
}

/// All pre-computed data needed for in-browser RAG.
#[derive(Debug, Clone, Deserialize)]
pub struct ChunkIndex {
    pub code_chunks: Vec<EmbeddedChunk<CodeChunk>>,
    pub readme_chunks: Vec<EmbeddedChunk<ReadmeChunk>>,
    pub crate_chunks: Vec<EmbeddedChunk<CrateChunk>>,
    pub module_doc_chunks: Vec<EmbeddedChunk<ModuleDocChunk>>,
    /// Pre-computed prototype embeddings for intent classification.
    /// Keys: "overview", "implementation", "relationship", "comparison"
    pub intent_prototypes: std::collections::HashMap<String, Vec<Vec<f32>>>,
    pub projects: Vec<String>,
}

/// Fetch and deserialize the pre-computed index from a static asset URL.
pub async fn load_index(url: &str) -> Result<ChunkIndex, String> {
    let resp = Request::get(url)
        .send()
        .await
        .map_err(|e| format!("Failed to fetch index: {e}"))?;

    if !resp.ok() {
        return Err(format!("Index fetch failed ({})", resp.status()));
    }

    resp.json::<ChunkIndex>()
        .await
        .map_err(|e| format!("Failed to parse index: {e}"))
}
