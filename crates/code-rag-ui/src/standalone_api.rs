//! Standalone API — runs engine in-browser instead of calling /chat endpoint.

use std::collections::HashMap;

use code_rag_engine::config::{RerankConfig, fetch_limits};
use code_rag_engine::context;
use code_rag_engine::intent::{
    self, ClassificationResult, IntentClassifier, QueryIntent, RoutingTable,
};
use code_rag_engine::retriever::{self, RerankText, RetrievalResult, ScoredChunk, sigmoid};

use crate::api::{ChatResponse, SourceInfo};
use crate::auth::AuthMethod;
use crate::data::ChunkIndex;
use crate::gemini;
use crate::reranker;
use crate::search;

/// Build IntentClassifier from pre-computed prototype embeddings in the index.
pub fn build_classifier(index: &ChunkIndex) -> IntentClassifier {
    let mut prototypes: HashMap<QueryIntent, Vec<Vec<f32>>> = HashMap::new();

    for (key, embeddings) in &index.intent_prototypes {
        let intent = match key.as_str() {
            "overview" => QueryIntent::Overview,
            "implementation" => QueryIntent::Implementation,
            "relationship" => QueryIntent::Relationship,
            "comparison" => QueryIntent::Comparison,
            _ => continue,
        };
        prototypes.insert(intent, embeddings.clone());
    }

    IntentClassifier::from_prototypes(prototypes)
}

/// Run RAG retrieval only (no LLM) — works without auth.
pub async fn send_chat_rag_only(
    query: &str,
    query_embedding: &[f32],
    index: &ChunkIndex,
    classifier: &IntentClassifier,
) -> Result<ChatResponse, String> {
    let (result, classification) = run_retrieval(query, query_embedding, index, classifier).await;
    let sources = build_source_list(&result);
    let intent_str = format_intent(classification.intent);

    let answer = format!(
        "<p>Found <strong>{}</strong> relevant sources. \
         Sign in with Google or provide an API key to get an AI-generated answer.</p>",
        sources.len()
    );

    Ok(ChatResponse {
        answer,
        sources,
        intent: intent_str,
    })
}

/// Run the full RAG pipeline in-browser and return a ChatResponse.
pub async fn send_chat_standalone(
    query: &str,
    query_embedding: &[f32],
    index: &ChunkIndex,
    classifier: &IntentClassifier,
    auth: &AuthMethod,
) -> Result<ChatResponse, String> {
    let (result, classification) = run_retrieval(query, query_embedding, index, classifier).await;

    let ctx = context::build_context(&result);
    let prompt = context::build_prompt(query, &ctx);
    let answer = gemini::generate(&prompt, auth).await?;

    let sources = build_source_list(&result);
    let intent_str = format_intent(classification.intent);

    Ok(ChatResponse {
        answer,
        sources,
        intent: intent_str,
    })
}

// --- Internal helpers ---

async fn run_retrieval(
    query: &str,
    query_embedding: &[f32],
    index: &ChunkIndex,
    classifier: &IntentClassifier,
) -> (RetrievalResult, ClassificationResult) {
    let routing = RoutingTable::default();
    let classification = intent::classify(query_embedding, classifier);
    let rerank_config = RerankConfig {
        enabled: true,
        ..Default::default()
    };

    // Only rerank intents where the cross-encoder improves results
    let should_rerank = matches!(
        classification.intent,
        QueryIntent::Implementation | QueryIntent::Overview
    );

    let final_config = intent::route(classification.intent, &routing);
    let search_config = if should_rerank {
        fetch_limits(&final_config, &rerank_config)
    } else {
        final_config.clone()
    };

    // Use hybrid search (BM25 + vector via RRF) when IDF data is available.
    // Hybrid results are relevance-scored (higher=better), vector-only returns distances (lower=better).
    let has_idf = index.code_idf.is_some();
    let (code_raw, readme_raw, crate_raw, module_doc_raw) = if has_idf {
        search::hybrid_search(query, query_embedding, index, &search_config)
    } else {
        search::brute_force_search(query_embedding, index, &search_config)
    };

    let result = if should_rerank {
        // Convert to scored chunks — hybrid scores are already relevance (higher=better),
        // vector distances need conversion.
        let (code_scored, readme_scored, crate_scored, module_doc_scored) = if has_idf {
            (
                retriever::to_scored_relevance(code_raw),
                retriever::to_scored_relevance(readme_raw),
                retriever::to_scored_relevance(crate_raw),
                retriever::to_scored_relevance(module_doc_raw),
            )
        } else {
            (
                retriever::to_scored(code_raw),
                retriever::to_scored(readme_raw),
                retriever::to_scored(crate_raw),
                retriever::to_scored(module_doc_raw),
            )
        };

        match rerank_all(
            query,
            code_scored,
            readme_scored,
            crate_scored,
            module_doc_scored,
            &final_config,
            classification.intent,
        )
        .await
        {
            Ok(result) => result,
            Err(e) => {
                web_sys::console::warn_1(
                    &format!("Reranking failed, using search scores: {e}").into(),
                );
                let (c, r, cr, m) = if has_idf {
                    search::hybrid_search(query, query_embedding, index, &final_config)
                } else {
                    search::brute_force_search(query_embedding, index, &final_config)
                };
                if has_idf {
                    RetrievalResult {
                        code_chunks: retriever::to_scored_relevance(c),
                        readme_chunks: retriever::to_scored_relevance(r),
                        crate_chunks: retriever::to_scored_relevance(cr),
                        module_doc_chunks: retriever::to_scored_relevance(m),
                        intent: classification.intent,
                    }
                } else {
                    retriever::to_retrieval_result(c, r, cr, m, classification.intent)
                }
            }
        }
    } else if has_idf {
        RetrievalResult {
            code_chunks: retriever::to_scored_relevance(code_raw),
            readme_chunks: retriever::to_scored_relevance(readme_raw),
            crate_chunks: retriever::to_scored_relevance(crate_raw),
            module_doc_chunks: retriever::to_scored_relevance(module_doc_raw),
            intent: classification.intent,
        }
    } else {
        retriever::to_retrieval_result(
            code_raw,
            readme_raw,
            crate_raw,
            module_doc_raw,
            classification.intent,
        )
    };

    (result, classification)
}

async fn rerank_chunks<T: RerankText + Clone>(
    query: &str,
    chunks: Vec<ScoredChunk<T>>,
    limit: usize,
) -> Result<Vec<ScoredChunk<T>>, String> {
    if chunks.is_empty() {
        return Ok(chunks);
    }

    let documents: Vec<String> = chunks.iter().map(|sc| sc.chunk.rerank_text()).collect();
    let rerank_results = reranker::rerank(query, documents).await?;

    let mut reranked: Vec<ScoredChunk<T>> = rerank_results
        .into_iter()
        .filter_map(|(index, score)| {
            chunks.get(index).map(|original| ScoredChunk {
                chunk: original.chunk.clone(),
                score: sigmoid(score),
            })
        })
        .collect();

    reranked.truncate(limit);
    Ok(reranked)
}

async fn rerank_all(
    query: &str,
    code: Vec<ScoredChunk<code_rag_types::CodeChunk>>,
    readme: Vec<ScoredChunk<code_rag_types::ReadmeChunk>>,
    crates: Vec<ScoredChunk<code_rag_types::CrateChunk>>,
    module_doc: Vec<ScoredChunk<code_rag_types::ModuleDocChunk>>,
    config: &code_rag_engine::config::RetrievalConfig,
    intent: QueryIntent,
) -> Result<RetrievalResult, String> {
    Ok(RetrievalResult {
        code_chunks: rerank_chunks(query, code, config.code_limit).await?,
        readme_chunks: rerank_chunks(query, readme, config.readme_limit).await?,
        crate_chunks: rerank_chunks(query, crates, config.crate_limit).await?,
        module_doc_chunks: rerank_chunks(query, module_doc, config.module_doc_limit).await?,
        intent,
    })
}

fn format_intent(intent: QueryIntent) -> String {
    serde_json::to_string(&intent)
        .unwrap_or_else(|_| "\"unknown\"".to_string())
        .trim_matches('"')
        .to_string()
}

fn build_source_list(result: &RetrievalResult) -> Vec<SourceInfo> {
    let mut sources: Vec<SourceInfo> = Vec::new();

    for s in &result.code_chunks {
        sources.push(SourceInfo {
            chunk_type: "code".into(),
            path: s.chunk.file_path.clone(),
            label: s.chunk.identifier.clone(),
            project: s.chunk.project_name.clone(),
            relevance: s.score,
            relevance_pct: (s.score * 100.0).round() as u8,
            line: s.chunk.start_line,
        });
    }
    for s in &result.readme_chunks {
        sources.push(SourceInfo {
            chunk_type: "readme".into(),
            path: s.chunk.file_path.clone(),
            label: s.chunk.project_name.clone(),
            project: s.chunk.project_name.clone(),
            relevance: s.score,
            relevance_pct: (s.score * 100.0).round() as u8,
            line: 0,
        });
    }
    for s in &result.crate_chunks {
        sources.push(SourceInfo {
            chunk_type: "crate".into(),
            path: s.chunk.crate_path.clone(),
            label: s.chunk.crate_name.clone(),
            project: s.chunk.project_name.clone(),
            relevance: s.score,
            relevance_pct: (s.score * 100.0).round() as u8,
            line: 0,
        });
    }
    for s in &result.module_doc_chunks {
        sources.push(SourceInfo {
            chunk_type: "module_doc".into(),
            path: s.chunk.file_path.clone(),
            label: s.chunk.module_name.clone(),
            project: s.chunk.project_name.clone(),
            relevance: s.score,
            relevance_pct: (s.score * 100.0).round() as u8,
            line: 0,
        });
    }

    sources.sort_by(|a, b| {
        b.relevance
            .partial_cmp(&a.relevance)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    sources
}
