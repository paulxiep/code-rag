// Re-export types from shared engine crate
pub use code_rag_engine::retriever::{RetrievalResult, to_retrieval_result};

use crate::store::VectorStore;
use code_rag_engine::config::RetrievalConfig;
use code_rag_engine::intent::QueryIntent;

use super::EngineError;

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

    let result = to_retrieval_result(code_raw, readme_raw, crate_raw, module_doc_raw, intent);

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
