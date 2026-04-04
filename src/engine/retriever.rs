// Re-export types from shared engine crate
pub use code_rag_engine::retriever::{RetrievalResult, to_retrieval_result};

use crate::store::{Reranker, VectorStore};
use code_rag_engine::config::{RerankConfig, RetrievalConfig, fetch_limits};
use code_rag_engine::intent::QueryIntent;
use code_rag_engine::retriever::{RerankText, ScoredChunk, sigmoid, to_scored};

use super::EngineError;

/// Rerank a vec of scored chunks using the cross-encoder.
/// Returns chunks re-sorted by sigmoid-normalized cross-encoder score, truncated to limit.
fn rerank_chunks<T: RerankText + Clone>(
    query: &str,
    chunks: Vec<ScoredChunk<T>>,
    reranker: &mut Reranker,
    limit: usize,
) -> Result<Vec<ScoredChunk<T>>, EngineError> {
    if chunks.is_empty() {
        return Ok(chunks);
    }

    let documents: Vec<String> = chunks.iter().map(|sc| sc.chunk.rerank_text()).collect();

    let rerank_results = reranker
        .rerank(query, documents)
        .map_err(|e| EngineError::Rerank(e.to_string()))?;

    let mut reranked: Vec<ScoredChunk<T>> = rerank_results
        .into_iter()
        .filter_map(|rr| {
            chunks.get(rr.index).map(|original| ScoredChunk {
                chunk: original.chunk.clone(),
                score: sigmoid(rr.score),
            })
        })
        .collect();

    reranked.truncate(limit);
    Ok(reranked)
}

/// Rerank all chunk types and build a RetrievalResult.
#[allow(clippy::too_many_arguments)]
fn rerank_all(
    query: &str,
    code: Vec<ScoredChunk<code_rag_types::CodeChunk>>,
    readme: Vec<ScoredChunk<code_rag_types::ReadmeChunk>>,
    crates: Vec<ScoredChunk<code_rag_types::CrateChunk>>,
    module_doc: Vec<ScoredChunk<code_rag_types::ModuleDocChunk>>,
    reranker: &mut Reranker,
    config: &RetrievalConfig,
    intent: QueryIntent,
) -> Result<RetrievalResult, EngineError> {
    Ok(RetrievalResult {
        code_chunks: rerank_chunks(query, code, reranker, config.code_limit)?,
        readme_chunks: rerank_chunks(query, readme, reranker, config.readme_limit)?,
        crate_chunks: rerank_chunks(query, crates, reranker, config.crate_limit)?,
        module_doc_chunks: rerank_chunks(query, module_doc, reranker, config.module_doc_limit)?,
        intent,
    })
}

/// Search vector store for similar chunks using a pre-computed query embedding.
/// When reranking is enabled, over-retrieves then re-scores with the cross-encoder.
pub async fn retrieve(
    query: &str,
    query_embedding: &[f32],
    store: &VectorStore,
    config: &RetrievalConfig,
    rerank_config: &RerankConfig,
    reranker: Option<&mut Reranker>,
    intent: QueryIntent,
) -> Result<RetrievalResult, EngineError> {
    let fetch_config = fetch_limits(config, rerank_config);

    let (code_raw, readme_raw, crate_raw, module_doc_raw) = store
        .search_all(
            query_embedding,
            fetch_config.code_limit,
            fetch_config.readme_limit,
            fetch_config.crate_limit,
            fetch_config.module_doc_limit,
        )
        .await?;

    // Only rerank intents where the cross-encoder improves results.
    // ms-marco-MiniLM-L-6-v2 hurts relationship (-0.26) and comparison (-0.06) queries
    // because it misjudges structural/relational relevance as a web-passage model.
    let should_rerank = rerank_config.enabled
        && matches!(intent, QueryIntent::Implementation | QueryIntent::Overview);

    let result = if should_rerank {
        if let Some(reranker) = reranker {
            let code_scored = to_scored(code_raw);
            let readme_scored = to_scored(readme_raw);
            let crate_scored = to_scored(crate_raw);
            let module_doc_scored = to_scored(module_doc_raw);

            // Attempt reranking; on failure, fall back to distance-based scores
            // (already converted via to_scored, just truncate to final limits)
            match rerank_all(
                query,
                code_scored,
                readme_scored,
                crate_scored,
                module_doc_scored,
                reranker,
                config,
                intent,
            ) {
                Ok(result) => result,
                Err(e) => {
                    tracing::warn!("reranking failed, falling back to distance scores: {e}");
                    // Re-fetch without over-retrieval for fallback
                    let (c, r, cr, m) = store
                        .search_all(
                            query_embedding,
                            config.code_limit,
                            config.readme_limit,
                            config.crate_limit,
                            config.module_doc_limit,
                        )
                        .await?;
                    to_retrieval_result(c, r, cr, m, intent)
                }
            }
        } else {
            tracing::warn!("reranking enabled but no reranker available, using distance scores");
            to_retrieval_result(code_raw, readme_raw, crate_raw, module_doc_raw, intent)
        }
    } else {
        to_retrieval_result(code_raw, readme_raw, crate_raw, module_doc_raw, intent)
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
        reranked = should_rerank,
        intent = ?result.intent,
        "retrieved"
    );

    Ok(result)
}
