use code_rag_types::{CodeChunk, CrateChunk, ModuleDocChunk, ReadmeChunk};

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
pub fn distance_to_relevance(dist: f32) -> f32 {
    1.0 / (1.0 + dist)
}

/// Convert (chunk, distance) pairs into scored chunks.
pub fn to_scored<T>(pairs: Vec<(T, f32)>) -> Vec<ScoredChunk<T>> {
    pairs
        .into_iter()
        .map(|(chunk, dist)| ScoredChunk {
            score: distance_to_relevance(dist),
            chunk,
        })
        .collect()
}

/// Build a RetrievalResult from raw search results (chunk + distance tuples).
pub fn to_retrieval_result(
    code_raw: Vec<(CodeChunk, f32)>,
    readme_raw: Vec<(ReadmeChunk, f32)>,
    crate_raw: Vec<(CrateChunk, f32)>,
    module_doc_raw: Vec<(ModuleDocChunk, f32)>,
    intent: QueryIntent,
) -> RetrievalResult {
    RetrievalResult {
        code_chunks: to_scored(code_raw),
        readme_chunks: to_scored(readme_raw),
        crate_chunks: to_scored(crate_raw),
        module_doc_chunks: to_scored(module_doc_raw),
        intent,
    }
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
