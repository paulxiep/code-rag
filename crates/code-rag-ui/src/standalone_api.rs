//! Standalone API — runs engine in-browser instead of calling /chat endpoint.

use std::collections::HashMap;

use code_rag_engine::comparison::fuse_comparator_lists;
use code_rag_engine::config::{RerankConfig, fetch_limits};
use code_rag_engine::context;
use code_rag_engine::graph;
use code_rag_engine::intent::{
    self, ClassificationResult, IntentClassifier, QueryIntent, RoutingTable, arm_policy,
};
use code_rag_engine::retriever::{self, RerankText, RetrievalResult, ScoredChunk, sigmoid};

use crate::api::{ChatResponse, SourceInfo};
use crate::auth::AuthMethod;
use crate::data::ChunkIndex;
use crate::embedder::embed_query;
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
    // Keyword pre-filter for unambiguous comparison cues (hard override).
    let classification = if let Some(pre) = intent::pre_classify_comparison(query) {
        ClassificationResult {
            intent: pre,
            confidence: 1.0,
            margin: 0.0,
        }
    } else {
        intent::classify(query_embedding, classifier)
    };
    let rerank_config = RerankConfig {
        enabled: true,
        ..Default::default()
    };

    // B5: per-intent arm policy + bundle capability checks.
    let policy = arm_policy(classification.intent);
    let should_rerank = policy.rerank;
    let use_hybrid = policy.bm25 && index.code_idf.is_some();
    let sig_vec_available = index
        .code_chunks
        .iter()
        .any(|c| c.signature_embedding.is_some());
    let use_sig_vec = policy.sig_vec && sig_vec_available;

    let final_config = intent::route(classification.intent, &routing);
    let search_config = if should_rerank {
        fetch_limits(&final_config, &rerank_config)
    } else {
        final_config.clone()
    };

    // C3: For Comparison intent with extractable comparators, run one search per
    // comparator (with the comparator name prepended to the original query) and
    // RRF-fuse the results so both halves of the comparison get representation in
    // the final top-K. Project enforcement: post-filter every comparator's
    // candidate list on the project_name of the original-query top-1 result, so
    // augmented sub-queries can never pull chunks from a sibling project (the
    // demo bundle is multi-project; cross-project comparisons are nonsense).
    // Falls through to the standard single-arm path when extraction fails.
    let comparison_fused: Option<Vec<(code_rag_types::CodeChunk, f32)>> =
        if classification.intent == QueryIntent::Comparison {
            let comparators = intent::extract_comparators(query);
            if comparators.len() >= 2 {
                // 1. Original-query search — establishes target project AND
                //    contributes its own ranking to the fused result.
                // SOTA NOTE: see server retriever for the empirical record on
                // bare-comparator + BM25 sub-searches. Both regressed on this
                // BGE-small + multi-project corpus; sticking with body-vec-only
                // sub-searches.
                let (original_hits, _) =
                    search::search_code_arm(query, query_embedding, index, 12, false, false);
                // Vote-based project detection across the top-5 of the
                // original results: top-1 alone is too brittle when the query
                // matches a comparison-shaped function in an unrelated project.
                let target_project: Option<String> = {
                    let mut counts: std::collections::HashMap<&str, usize> =
                        std::collections::HashMap::new();
                    for (c, _) in original_hits.iter().take(5) {
                        *counts.entry(c.project_name.as_str()).or_insert(0) += 1;
                    }
                    counts
                        .into_iter()
                        .max_by_key(|(_, n)| *n)
                        .map(|(p, _)| p.to_string())
                };

                // 2. Per-comparator: await embed_query, then sync search.
                let mut lists: Vec<Vec<(code_rag_types::CodeChunk, f32)>> =
                    Vec::with_capacity(comparators.len() + 1);
                for comp in &comparators {
                    // Concatenated augmentation `{comparator} {original query}`
                    // — bare comparators embed poorly with BGE-small. Keep
                    // the full query as context and let the leading
                    // comparator token steer the embedding.
                    let sub_query = format!("{} {}", comp, query);
                    let emb = match embed_query(&sub_query).await {
                        Ok(e) if !e.is_empty() => e,
                        _ => continue,
                    };
                    let (raw, _is_rel) = search::search_code_arm(
                        &sub_query,
                        &emb,
                        index,
                        12,
                        false,
                        false,
                    );
                    let filtered: Vec<_> = raw
                        .into_iter()
                        .filter(|(c, _)| {
                            target_project
                                .as_ref()
                                .is_none_or(|p| &c.project_name == p)
                        })
                        .take(8)
                        .collect();
                    lists.push(filtered);
                }
                let original_filtered: Vec<_> = original_hits
                    .into_iter()
                    .filter(|(c, _)| {
                        target_project
                            .as_ref()
                            .is_none_or(|p| &c.project_name == p)
                    })
                    .take(8)
                    .collect();
                lists.push(original_filtered);

                // Build a max-score lookup from the natural per-list scores,
                // then assign each fused survivor its best natural score so
                // the global flatten/sort across code+readme+crate+module_doc
                // remains score-comparable. RRF decides ordering; max-of-
                // natural keeps the magnitude in the same range as the
                // distance-converted non-code arms. See server retriever for
                // the full rationale.
                let mut max_score: std::collections::HashMap<String, f32> =
                    std::collections::HashMap::new();
                for list in &lists {
                    for (c, s) in list {
                        let entry = max_score.entry(c.chunk_id.clone()).or_insert(f32::MIN);
                        if *s > *entry {
                            *entry = *s;
                        }
                    }
                }
                let fused = fuse_comparator_lists(lists, final_config.code_limit);
                let rescored: Vec<(code_rag_types::CodeChunk, f32)> = fused
                    .into_iter()
                    .map(|(c, _rrf)| {
                        let s = max_score.get(&c.chunk_id).copied().unwrap_or(0.5);
                        (c, s)
                    })
                    .collect();
                Some(rescored)
            } else {
                None
            }
        } else {
            None
        };

    let (code_raw, code_is_relevance) = if let Some(fused) = comparison_fused.as_ref() {
        // Synthetic rank-based scores (higher = better) — caller must use
        // `to_scored_relevance`, not the distance-converting `to_scored`.
        (fused.clone(), true)
    } else {
        search::search_code_arm(
            query,
            query_embedding,
            index,
            search_config.code_limit,
            use_hybrid,
            use_sig_vec,
        )
    };
    let (readme_raw, crate_raw, module_doc_raw) = if use_hybrid {
        search::hybrid_search_non_code(query, query_embedding, index, &search_config)
    } else {
        search::brute_force_non_code(query_embedding, index, &search_config)
    };

    // Normalize all four arms to ScoredChunk. Code uses relevance or distance
    // based on whether fusion happened; non-code uses relevance (hybrid) or
    // distance (brute-force).
    let code_scored = if code_is_relevance {
        retriever::to_scored_relevance(code_raw)
    } else {
        retriever::to_scored(code_raw)
    };

    // C1: Graph augmentation for Relationship + Implementation intents.
    // C2: augment_with_graph_wasm returns (merged, graph_ids). graph_ids is
    // threaded to reserve_graph_slots post-rerank (Relationship only).
    let (code_scored, graph_ids) = if !index.call_edges.is_empty()
        && (classification.intent == QueryIntent::Relationship
            || classification.intent == QueryIntent::Implementation)
    {
        augment_with_graph_wasm(query, code_scored, index)
    } else {
        (code_scored, std::collections::HashSet::new())
    };

    let (readme_scored, crate_scored, module_doc_scored) = if use_hybrid {
        (
            retriever::to_scored_relevance(readme_raw),
            retriever::to_scored_relevance(crate_raw),
            retriever::to_scored_relevance(module_doc_raw),
        )
    } else {
        (
            retriever::to_scored(readme_raw),
            retriever::to_scored(crate_raw),
            retriever::to_scored(module_doc_raw),
        )
    };

    // C2: SOTA routing for explicit-direction queries. See the server path
    // (src/engine/retriever.rs) for the full rationale. Summary: when the
    // query has explicit "what calls X / called by / uses" keywords, we
    // partition graph-confirmed chunks OUT of the rerank pipeline entirely
    // and prepend them (sorted by tier score) to the reranked non-graph
    // chunks. The reranker never had authority over structural hits, so
    // they cannot be demoted. Matches Cody/LocAgent/GraphCoder routing.
    let direction = graph::detect_direction(query);
    let explicit_structural =
        !graph_ids.is_empty() && direction != graph::GraphDirection::Both;

    let (graph_reserved, code_for_rerank): (
        Vec<ScoredChunk<code_rag_types::CodeChunk>>,
        Vec<ScoredChunk<code_rag_types::CodeChunk>>,
    ) = if explicit_structural && should_rerank {
        let (mut g, c): (Vec<_>, Vec<_>) = code_scored
            .iter()
            .cloned()
            .partition(|sc| graph_ids.contains(&sc.chunk.chunk_id));
        g.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let max_graph_slots = final_config.code_limit.saturating_sub(1);
        g.truncate(max_graph_slots);
        (g, c)
    } else {
        (Vec::new(), code_scored.clone())
    };

    // Soft-reserve only applies when we're NOT routing (direction == Both).
    let soft_reserve_active = !explicit_structural
        && !graph_ids.is_empty()
        && (classification.intent == QueryIntent::Relationship
            || classification.intent == QueryIntent::Implementation);
    let code_keep_override = if soft_reserve_active {
        Some(final_config.code_limit + 5)
    } else {
        None
    };

    let mut result = if should_rerank {
        match rerank_all(
            query,
            code_for_rerank.clone(),
            readme_scored.clone(),
            crate_scored.clone(),
            module_doc_scored.clone(),
            &final_config,
            classification.intent,
            code_keep_override,
        )
        .await
        {
            Ok(result) => result,
            Err(e) => {
                web_sys::console::warn_1(
                    &format!("Reranking failed, using search scores: {e}").into(),
                );
                // Fallback: re-run searches with the non-over-retrieved config.
                let (c_raw, c_is_rel) = search::search_code_arm(
                    query,
                    query_embedding,
                    index,
                    final_config.code_limit,
                    use_hybrid,
                    use_sig_vec,
                );
                let (r, cr, m) = if use_hybrid {
                    search::hybrid_search_non_code(query, query_embedding, index, &final_config)
                } else {
                    search::brute_force_non_code(query_embedding, index, &final_config)
                };
                let code_chunks = if c_is_rel {
                    retriever::to_scored_relevance(c_raw)
                } else {
                    retriever::to_scored(c_raw)
                };
                if use_hybrid {
                    RetrievalResult {
                        code_chunks,
                        readme_chunks: retriever::to_scored_relevance(r),
                        crate_chunks: retriever::to_scored_relevance(cr),
                        module_doc_chunks: retriever::to_scored_relevance(m),
                        intent: classification.intent,
                    }
                } else {
                    RetrievalResult {
                        code_chunks,
                        readme_chunks: retriever::to_scored(r),
                        crate_chunks: retriever::to_scored(cr),
                        module_doc_chunks: retriever::to_scored(m),
                        intent: classification.intent,
                    }
                }
            }
        }
    } else {
        RetrievalResult {
            code_chunks: code_scored,
            readme_chunks: readme_scored,
            crate_chunks: crate_scored,
            module_doc_chunks: module_doc_scored,
            intent: classification.intent,
        }
    };

    // C2: stitch graph_reserved back in (explicit routing) or apply soft
    // reserve (ambiguous direction). Same logic as server path.
    if explicit_structural {
        let semantic_slots = final_config.code_limit.saturating_sub(graph_reserved.len());
        let mut combined: Vec<ScoredChunk<code_rag_types::CodeChunk>> = graph_reserved;
        combined.extend(
            std::mem::take(&mut result.code_chunks)
                .into_iter()
                .take(semantic_slots),
        );
        result.code_chunks = combined;
    } else if soft_reserve_active {
        let min_slots = if classification.intent == QueryIntent::Relationship {
            2
        } else {
            1
        };
        result.code_chunks = graph::reserve_graph_slots(
            std::mem::take(&mut result.code_chunks),
            &graph_ids,
            final_config.code_limit,
            min_slots,
        );
    }

    (result, classification)
}

/// C1: Graph augmentation for Relationship intent (WASM path).
/// Uses the same shared `graph::graph_augment` and `graph::merge_graph_chunks` as the server.
///
/// Returns `(merged_chunks, graph_ids)` — identical contract to the server-side
/// `augment_with_graph`. `graph_ids` includes vector/graph collisions so that
/// `reserve_graph_slots` can protect them post-rerank (C2).
fn augment_with_graph_wasm(
    query: &str,
    code_scored: Vec<ScoredChunk<code_rag_types::CodeChunk>>,
    index: &ChunkIndex,
) -> (
    Vec<ScoredChunk<code_rag_types::CodeChunk>>,
    std::collections::HashSet<String>,
) {
    // Extract top-5 candidates
    let candidates: Vec<(String, String)> = code_scored
        .iter()
        .take(5)
        .map(|sc| (sc.chunk.chunk_id.clone(), sc.chunk.identifier.clone()))
        .collect();

    if candidates.is_empty() {
        return (code_scored, std::collections::HashSet::new());
    }

    // Build CallGraph from ExportEdge data + register identifiers from chunk index
    let mut call_graph = graph::CallGraph::from_edges(
        index
            .call_edges
            .iter()
            .map(|e| (e.caller.clone(), e.callee.clone())),
    );
    // Register identifier → chunk_id pairs from all code chunks that appear in edges
    call_graph.register_identifiers(
        index
            .code_chunks
            .iter()
            .map(|ec| (ec.chunk.identifier.clone(), ec.chunk.chunk_id.clone())),
    );

    // Run shared graph augmentation logic (same as server)
    let augment_result = match graph::graph_augment(query, &candidates, &call_graph) {
        Some(r) => r,
        None => return (code_scored, std::collections::HashSet::new()),
    };

    // Look up full chunks via chunk_id index (WASM-specific: in-memory lookup)
    let graph_scored: Vec<ScoredChunk<code_rag_types::CodeChunk>> = augment_result
        .resolved_chunk_ids
        .iter()
        .filter_map(|cid| {
            let idx = index.chunk_id_index.get(cid)?;
            let ec = index.code_chunks.get(*idx)?;
            // Find the tier for this edge
            let tier = index
                .call_edges
                .iter()
                .find(|e| e.caller == *cid || e.callee == *cid)
                .map(|e| e.tier)
                .unwrap_or(3);
            Some(ScoredChunk {
                chunk: ec.chunk.clone(),
                score: graph::tier_score(tier),
            })
        })
        .collect();

    // Merge using shared collision-safe logic (same as server)
    graph::merge_graph_chunks(code_scored, graph_scored)
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
    code_keep_override: Option<usize>,
) -> Result<RetrievalResult, String> {
    let code_limit = code_keep_override.unwrap_or(config.code_limit);
    Ok(RetrievalResult {
        code_chunks: rerank_chunks(query, code, code_limit).await?,
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
