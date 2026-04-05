// Re-export types from shared engine crate
pub use code_rag_engine::retriever::{RetrievalResult, to_retrieval_result};

use crate::store::{Reranker, VectorStore};
use code_rag_engine::config::{
    DualEmbeddingConfig, HybridConfig, RerankConfig, RetrievalConfig, fetch_limits,
};
use code_rag_engine::fusion::rrf_fuse;
use code_rag_engine::intent::{QueryIntent, arm_policy};
use code_rag_engine::retriever::{
    RerankText, ScoredChunk, sigmoid, to_scored, to_scored_relevance,
};

use super::EngineError;

/// Fetch the code arm, respecting hybrid + dual-embedding toggles.
/// Returns (chunk, relevance_score) tuples where higher=better so downstream
/// `to_scored_relevance` can wrap them uniformly.
async fn fetch_code_arm(
    store: &VectorStore,
    query: &str,
    query_embedding: &[f32],
    limit: usize,
    use_hybrid: bool,
    use_sig_vec: bool,
) -> Result<Vec<(code_rag_types::CodeChunk, f32)>, code_rag_store::StoreError> {
    // Body arm: either hybrid (body-vec + BM25 fused inside LanceDB) or pure body-vec.
    // Normalize to higher=better relevance scores.
    let body: Vec<(code_rag_types::CodeChunk, f32)> = if use_hybrid {
        store.hybrid_search_code(query, query_embedding, limit).await?
    } else {
        store
            .search_code(query_embedding, limit)
            .await?
            .into_iter()
            .map(|(c, d)| (c, 1.0 / (1.0 + d)))
            .collect()
    };

    if !use_sig_vec {
        return Ok(body);
    }

    // Signature arm: pure vector search over signature_vector column.
    let sig: Vec<(code_rag_types::CodeChunk, f32)> = store
        .search_code_signatures(query_embedding, limit)
        .await?
        .into_iter()
        .map(|(c, d)| (c, 1.0 / (1.0 + d)))
        .collect();

    // App-level RRF fusion. rrf_fuse ignores the input scores and fuses by rank.
    Ok(rrf_fuse(&[body, sig], 60, |c: &code_rag_types::CodeChunk| {
        c.chunk_id.as_str()
    }))
}

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
/// When hybrid is enabled, uses BM25+semantic via RRF fusion (scores are higher=better).
#[allow(clippy::too_many_arguments)]
pub async fn retrieve(
    query: &str,
    query_embedding: &[f32],
    store: &VectorStore,
    config: &RetrievalConfig,
    rerank_config: &RerankConfig,
    hybrid_config: &HybridConfig,
    dual_embedding_config: &DualEmbeddingConfig,
    reranker: Option<&mut Reranker>,
    intent: QueryIntent,
) -> Result<RetrievalResult, EngineError> {
    // B5: per-intent arm policy is combined with global config toggles.
    // The policy captures intent-specific empirical findings; the config
    // flags capture the global feature toggle.
    let policy = arm_policy(intent);
    let should_rerank = rerank_config.enabled && policy.rerank;
    let use_hybrid = hybrid_config.enabled && policy.bm25;
    let use_sig_vec = dual_embedding_config.enabled && policy.sig_vec;

    // Fetch multiplier only applies when we actually rerank — over-retrieval changes
    // top-K ordering for vector search (larger pool shifts which 5 are closest).
    // B5: the multiplier applies to the code arm for every active sub-arm so RRF
    // sees a richer pool on each axis.
    let fetch_config = if should_rerank {
        fetch_limits(config, rerank_config)
    } else {
        config.clone()
    };

    // Code arm is orchestrated at the app level to support dual-vector fusion.
    let code_raw =
        fetch_code_arm(
            store,
            query,
            query_embedding,
            fetch_config.code_limit,
            use_hybrid,
            use_sig_vec,
        )
        .await?;
    let code_scored = to_scored_relevance(code_raw);

    // Non-code tables are untouched by B5 — they follow the hybrid toggle only.
    let (readme_scored, crate_scored, module_doc_scored) = if use_hybrid {
        let (readme_raw, crate_raw, module_doc_raw) = tokio::try_join!(
            store.hybrid_search_readme(query, query_embedding, fetch_config.readme_limit),
            store.hybrid_search_crates(query, query_embedding, fetch_config.crate_limit),
            store.hybrid_search_module_docs(
                query,
                query_embedding,
                fetch_config.module_doc_limit
            ),
        )?;
        (
            to_scored_relevance(readme_raw),
            to_scored_relevance(crate_raw),
            to_scored_relevance(module_doc_raw),
        )
    } else {
        let (readme_raw, crate_raw, module_doc_raw) = tokio::try_join!(
            store.search_readme(query_embedding, fetch_config.readme_limit),
            store.search_crates(query_embedding, fetch_config.crate_limit),
            store.search_module_docs(query_embedding, fetch_config.module_doc_limit),
        )?;
        (
            to_scored(readme_raw),
            to_scored(crate_raw),
            to_scored(module_doc_raw),
        )
    };

    let result = if should_rerank {
        if let Some(reranker) = reranker {
            // Attempt reranking; on failure, fall back to current scores
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
                    tracing::warn!("reranking failed, falling back to search scores: {e}");
                    // Re-fetch without over-retrieval for fallback. Use the same
                    // dual-arm orchestrator so behavior is consistent with the
                    // primary path.
                    let code_raw = fetch_code_arm(
                        store,
                        query,
                        query_embedding,
                        config.code_limit,
                        use_hybrid,
                        use_sig_vec,
                    )
                    .await?;
                    let (readme_raw, crate_raw, module_doc_raw, code_chunks) = if use_hybrid {
                        let (r, cr, m) = tokio::try_join!(
                            store.hybrid_search_readme(
                                query,
                                query_embedding,
                                config.readme_limit
                            ),
                            store.hybrid_search_crates(
                                query,
                                query_embedding,
                                config.crate_limit
                            ),
                            store.hybrid_search_module_docs(
                                query,
                                query_embedding,
                                config.module_doc_limit
                            ),
                        )?;
                        (
                            to_scored_relevance(r),
                            to_scored_relevance(cr),
                            to_scored_relevance(m),
                            to_scored_relevance(code_raw),
                        )
                    } else {
                        let (r, cr, m) = tokio::try_join!(
                            store.search_readme(query_embedding, config.readme_limit),
                            store.search_crates(query_embedding, config.crate_limit),
                            store.search_module_docs(query_embedding, config.module_doc_limit),
                        )?;
                        (
                            to_scored(r),
                            to_scored(cr),
                            to_scored(m),
                            to_scored_relevance(code_raw),
                        )
                    };
                    RetrievalResult {
                        code_chunks,
                        readme_chunks: readme_raw,
                        crate_chunks: crate_raw,
                        module_doc_chunks: module_doc_raw,
                        intent,
                    }
                }
            }
        } else {
            tracing::warn!("reranking enabled but no reranker available, using search scores");
            RetrievalResult {
                code_chunks: code_scored,
                readme_chunks: readme_scored,
                crate_chunks: crate_scored,
                module_doc_chunks: module_doc_scored,
                intent,
            }
        }
    } else {
        RetrievalResult {
            code_chunks: code_scored,
            readme_chunks: readme_scored,
            crate_chunks: crate_scored,
            module_doc_chunks: module_doc_scored,
            intent,
        }
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
