use crate::models::{CodeChunk, CrateChunk, ModuleDocChunk, ReadmeChunk};
use crate::store::VectorStore;

use super::EngineError;
use super::config::RetrievalConfig;
use super::intent::QueryIntent;

/// A chunk paired with its relevance score (0.0–1.0, higher = more relevant).
#[derive(Debug, Clone)]
pub struct ScoredChunk<T> {
    pub chunk: T,
    pub score: f32,
}

/// Retrieved context from vector search, with scores and classified intent.
#[derive(Debug)]
pub struct RetrievalResult {
    pub code_chunks: Vec<ScoredChunk<CodeChunk>>,
    pub readme_chunks: Vec<ScoredChunk<ReadmeChunk>>,
    pub crate_chunks: Vec<ScoredChunk<CrateChunk>>,
    pub module_doc_chunks: Vec<ScoredChunk<ModuleDocChunk>>,
    pub intent: QueryIntent,
}

/// Convert L2 distance to relevance score.
/// Maps [0, ∞) → (0, 1]. Zero distance = perfect match (1.0).
fn distance_to_relevance(dist: f32) -> f32 {
    1.0 / (1.0 + dist)
}

fn to_scored<T>(pairs: Vec<(T, f32)>) -> Vec<ScoredChunk<T>> {
    pairs
        .into_iter()
        .map(|(chunk, dist)| ScoredChunk {
            score: distance_to_relevance(dist),
            chunk,
        })
        .collect()
}

/// Search vector store for similar chunks using a pre-computed query embedding.
pub async fn retrieve(
    query_embedding: &[f32],
    store: &VectorStore,
    config: &RetrievalConfig,
    intent: QueryIntent,
) -> Result<RetrievalResult, EngineError> {
    let (code_raw, readme_raw, crate_raw, module_doc_raw) = store
        .search_all(
            query_embedding,
            config.code_limit,
            config.readme_limit,
            config.crate_limit,
            config.module_doc_limit,
        )
        .await?;

    let result = RetrievalResult {
        code_chunks: to_scored(code_raw),
        readme_chunks: to_scored(readme_raw),
        crate_chunks: to_scored(crate_raw),
        module_doc_chunks: to_scored(module_doc_raw),
        intent,
    };

    let total = result.code_chunks.len()
        + result.readme_chunks.len()
        + result.crate_chunks.len()
        + result.module_doc_chunks.len();
    let top_relevance = result
        .code_chunks
        .iter()
        .map(|s| s.score)
        .chain(result.readme_chunks.iter().map(|s| s.score))
        .chain(result.crate_chunks.iter().map(|s| s.score))
        .chain(result.module_doc_chunks.iter().map(|s| s.score))
        .fold(0.0_f32, f32::max);

    tracing::info!(
        sources = total,
        code = result.code_chunks.len(),
        readme = result.readme_chunks.len(),
        crates = result.crate_chunks.len(),
        module_doc = result.module_doc_chunks.len(),
        top_relevance,
        intent = ?result.intent,
        "retrieved"
    );

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_distance_to_relevance_zero() {
        let score = distance_to_relevance(0.0);
        assert!((score - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_distance_to_relevance_one() {
        let score = distance_to_relevance(1.0);
        assert!((score - 0.5).abs() < 1e-6);
    }

    #[test]
    fn test_distance_to_relevance_large() {
        let score = distance_to_relevance(100.0);
        assert!(score < 0.02);
        assert!(score > 0.0);
    }
}
