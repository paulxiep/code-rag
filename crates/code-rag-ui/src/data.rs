//! Load pre-computed chunk index from static JSON asset.

use code_rag_engine::text::build_searchable_text;
use gloo_net::http::Request;
use serde::Deserialize;

use code_rag_types::{CodeChunk, CrateChunk, ExportEdge, ModuleDocChunk, ReadmeChunk};

/// A chunk paired with its pre-computed embedding vector.
#[derive(Debug, Clone, Deserialize)]
pub struct EmbeddedChunk<T> {
    #[serde(flatten)]
    pub chunk: T,
    pub embedding: Vec<f32>,
    /// B5: signature-text embedding. Only populated for code chunks that have
    /// a signature. Non-code chunks and signature-less code chunks deserialize
    /// with None via `#[serde(default)]`.
    #[serde(default)]
    pub signature_embedding: Option<Vec<f32>>,
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
    /// Pre-computed IDF tables for browser-side BM25 (B2).
    #[serde(default)]
    pub code_idf: Option<super::text_search::IdfTable>,
    #[serde(default)]
    pub readme_idf: Option<super::text_search::IdfTable>,
    #[serde(default)]
    pub crate_idf: Option<super::text_search::IdfTable>,
    #[serde(default)]
    pub module_doc_idf: Option<super::text_search::IdfTable>,

    /// C1: Call graph edges for browser-side graph traversal.
    #[serde(default)]
    pub call_edges: Vec<ExportEdge>,

    /// Pre-computed searchable_text for code chunks (B3).
    /// Built at load time from identifier + signature + docstring.
    #[serde(skip)]
    pub code_searchable_texts: Vec<String>,

    /// C1: chunk_id → index into code_chunks vec for O(1) graph-resolved lookups.
    #[serde(skip)]
    pub chunk_id_index: std::collections::HashMap<String, usize>,
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

    let mut index: ChunkIndex = resp
        .json::<ChunkIndex>()
        .await
        .map_err(|e| format!("Failed to parse index: {e}"))?;

    // B3: Pre-compute searchable_text for code chunks at load time
    index.code_searchable_texts = index
        .code_chunks
        .iter()
        .map(|ec| {
            build_searchable_text(
                &ec.chunk.identifier,
                ec.chunk.signature.as_deref(),
                ec.chunk.docstring.as_deref(),
            )
        })
        .collect();

    // C1: Build chunk_id → index lookup for O(1) graph-resolved lookups
    index.chunk_id_index = index
        .code_chunks
        .iter()
        .enumerate()
        .map(|(i, ec)| (ec.chunk.chunk_id.clone(), i))
        .collect();

    Ok(index)
}
