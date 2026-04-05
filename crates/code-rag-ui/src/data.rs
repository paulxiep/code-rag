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

    /// Pre-computed searchable_text for code chunks (B3).
    /// Built at load time from identifier + signature + docstring.
    #[serde(skip)]
    pub code_searchable_texts: Vec<String>,
}

/// Build searchable_text from high-signal fields (mirrors server-side build_searchable_text).
/// Identifier boosted 2x + camelCase split + signature + docstring.
fn build_searchable_text(identifier: &str, signature: Option<&str>, docstring: Option<&str>) -> String {
    let mut parts = Vec::new();
    let split = split_camel_case(identifier);
    if split != identifier.to_lowercase() {
        parts.push(format!("{} {} {}", identifier, identifier, split));
    } else {
        parts.push(format!("{} {}", identifier, identifier));
    }
    if let Some(sig) = signature {
        parts.push(sig.to_string());
    }
    if let Some(doc) = docstring {
        if !doc.is_empty() {
            parts.push(doc.to_string());
        }
    }
    parts.join("\n")
}

/// Split camelCase/PascalCase into lowercase words.
fn split_camel_case(s: &str) -> String {
    let mut words = Vec::new();
    let mut current = String::new();
    let chars: Vec<char> = s.chars().collect();
    for i in 0..chars.len() {
        let c = chars[i];
        if c.is_uppercase() && !current.is_empty() {
            let prev_upper = i > 0 && chars[i - 1].is_uppercase();
            let next_lower = i + 1 < chars.len() && chars[i + 1].is_lowercase();
            if !prev_upper || next_lower {
                words.push(current.to_lowercase());
                current = String::new();
            }
        }
        current.push(c);
    }
    if !current.is_empty() {
        words.push(current.to_lowercase());
    }
    words.join(" ")
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

    Ok(index)
}
