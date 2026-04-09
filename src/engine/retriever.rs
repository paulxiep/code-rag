// Re-export types from shared engine crate
pub use code_rag_engine::retriever::{RetrievalResult, to_retrieval_result};

use crate::store::{Embedder, Reranker, VectorStore};
use code_rag_engine::comparison::fuse_comparator_lists;
use code_rag_engine::config::{EngineConfig, RetrievalConfig, fetch_limits};
use code_rag_engine::fusion::rrf_fuse;
use code_rag_engine::graph;
use code_rag_engine::intent::{self, QueryIntent, arm_policy};
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
        store
            .hybrid_search_code(query, query_embedding, limit)
            .await?
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
    Ok(rrf_fuse(
        &[body, sig],
        60,
        |c: &code_rag_types::CodeChunk| c.chunk_id.as_str(),
    ))
}

/// C1: Augment code search results with graph-resolved chunks for Relationship intent.
/// Uses shared `graph::graph_augment` and `graph::merge_graph_chunks` from code-rag-engine.
///
/// Returns `(merged_chunks, graph_ids)`. `graph_ids` is the set of every
/// chunk_id that the graph resolved — including ones already present in the
/// vector results. Callers pass it to `graph::reserve_graph_slots` after
/// reranking to protect structurally confirmed chunks from demotion (C2).
async fn augment_with_graph(
    query: &str,
    code_scored: Vec<ScoredChunk<code_rag_types::CodeChunk>>,
    store: &VectorStore,
) -> (
    Vec<ScoredChunk<code_rag_types::CodeChunk>>,
    std::collections::HashSet<String>,
) {
    // Extract top-5 candidates as (chunk_id, identifier) pairs
    let candidates: Vec<(String, String)> = code_scored
        .iter()
        .take(5)
        .map(|sc| (sc.chunk.chunk_id.clone(), sc.chunk.identifier.clone()))
        .collect();

    if candidates.is_empty() {
        return (code_scored, std::collections::HashSet::new());
    }

    // Infer project from top-1 result
    let project = &code_scored[0].chunk.project_name;

    // Load all edges for this project and build CallGraph
    let edges = match store.get_all_edges(project).await {
        Ok(e) => e,
        Err(_) => return (code_scored, std::collections::HashSet::new()),
    };

    if edges.is_empty() {
        return (code_scored, std::collections::HashSet::new());
    }

    // Build a tier lookup for scoring
    let tier_by_edge: std::collections::HashMap<(String, String), u8> = edges
        .iter()
        .map(|e| {
            (
                (e.caller_chunk_id.clone(), e.callee_chunk_id.clone()),
                e.resolution_tier,
            )
        })
        .collect();

    // Build identifier → chunk_id pairs from edges for graph target lookup
    let id_pairs: Vec<(String, String)> = edges
        .iter()
        .flat_map(|e| {
            vec![
                (e.caller_identifier.clone(), e.caller_chunk_id.clone()),
                (e.callee_identifier.clone(), e.callee_chunk_id.clone()),
            ]
        })
        .collect();

    let mut call_graph = graph::CallGraph::from_edges(
        edges
            .into_iter()
            .map(|e| (e.caller_chunk_id, e.callee_chunk_id)),
    );
    call_graph.register_identifiers(id_pairs);

    // Run shared graph augmentation logic
    let augment_result = match graph::graph_augment(query, &candidates, &call_graph) {
        Some(r) => r,
        None => return (code_scored, std::collections::HashSet::new()),
    };

    // Fetch full CodeChunks for graph-resolved IDs
    let graph_chunks = match store
        .get_chunks_by_ids(&augment_result.resolved_chunk_ids)
        .await
    {
        Ok(chunks) => chunks,
        Err(_) => return (code_scored, std::collections::HashSet::new()),
    };

    // Wrap as ScoredChunks with tier-based scores
    let graph_scored: Vec<ScoredChunk<code_rag_types::CodeChunk>> = graph_chunks
        .into_iter()
        .map(|chunk| {
            // Look up tier from the edge that resolved this chunk
            let tier = tier_by_edge
                .iter()
                .find(|((caller, callee), _)| {
                    *caller == chunk.chunk_id || *callee == chunk.chunk_id
                })
                .map(|(_, &t)| t)
                .unwrap_or(3);
            ScoredChunk {
                chunk,
                score: graph::tier_score(tier),
            }
        })
        .collect();

    // Merge using shared dedup logic
    graph::merge_graph_chunks(code_scored, graph_scored)
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
///
/// `code_keep_override` — when `Some(n)`, the code arm is reranked and
/// truncated to `n` chunks instead of `config.code_limit`. C2 uses this to
/// over-retain the code arm before `reserve_graph_slots` runs so the
/// promotion step has a below-cutoff buffer to rescue graph-confirmed chunks
/// from. Non-code arms always use their config limits. The `bundle.intent`
/// field is propagated to the returned `RetrievalResult` unchanged.
fn rerank_all(
    query: &str,
    bundle: RetrievalResult,
    reranker: &mut Reranker,
    config: &RetrievalConfig,
    code_keep_override: Option<usize>,
) -> Result<RetrievalResult, EngineError> {
    let code_limit = code_keep_override.unwrap_or(config.code_limit);
    Ok(RetrievalResult {
        code_chunks: rerank_chunks(query, bundle.code_chunks, reranker, code_limit)?,
        readme_chunks: rerank_chunks(query, bundle.readme_chunks, reranker, config.readme_limit)?,
        crate_chunks: rerank_chunks(query, bundle.crate_chunks, reranker, config.crate_limit)?,
        module_doc_chunks: rerank_chunks(
            query,
            bundle.module_doc_chunks,
            reranker,
            config.module_doc_limit,
        )?,
        intent: bundle.intent,
    })
}

/// Per-query inputs that travel together through the retrieval pipeline.
pub struct QueryContext<'a> {
    pub query: &'a str,
    pub embedding: &'a [f32],
    pub intent: QueryIntent,
}

/// Search vector store for similar chunks using a pre-computed query embedding.
/// When reranking is enabled, over-retrieves then re-scores with the cross-encoder.
/// When hybrid is enabled, uses BM25+semantic via RRF fusion (scores are higher=better).
pub async fn retrieve(
    qctx: QueryContext<'_>,
    store: &VectorStore,
    embedder: &mut Embedder,
    config: &RetrievalConfig,
    engine_config: &EngineConfig,
    reranker: Option<&mut Reranker>,
) -> Result<RetrievalResult, EngineError> {
    let QueryContext {
        query,
        embedding: query_embedding,
        intent,
    } = qctx;
    let rerank_config = &engine_config.rerank;
    let hybrid_config = &engine_config.hybrid;
    let dual_embedding_config = &engine_config.dual_embedding;

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
    // C3: For Comparison intent with extractable comparators, run one search per
    // comparator (with the comparator name prepended to the original query) and
    // RRF-fuse the results so both halves of the comparison get representation in
    // the final top-K. Falls through to the standard single-arm path when
    // extraction fails. Project enforcement: post-filter every comparator's
    // candidate list on the project_name of the original-query top-1 result, so
    // augmented sub-queries can never pull chunks from a sibling project (the
    // WASM demo bundle is multi-project; cross-project comparisons are nonsense).
    let code_raw = if intent == QueryIntent::Comparison {
        let comparators = intent::extract_comparators(query);
        if comparators.len() >= 2 {
            // 1. Original-query search — establishes target project AND contributes
            //    its own ranking to the fused result.
            // SOTA NOTE: LlamaIndex SubQuestionQueryEngine, RAG-Fusion, and
            // CodeRAG-Bench all recommend bare-comparator decomposition with
            // BM25-enabled sub-searches. We empirically tested both knobs on
            // this corpus (BGE-small-en-v1.5 + 3-project bundle) and BOTH
            // regressed comparison recall@5 vs the body-vec-only path:
            //   • bare comparator + body-vec only        → 0.31
            //   • bare comparator + hybrid + rank-norm   → 0.54
            //   • concatenated + hybrid + rank-norm      → 0.44
            //   • concatenated + body-vec only           → 0.65 (chosen)
            // The B3 "BM25 over-ranks one half of a pair" finding generalizes
            // to per-comparator sub-searches on this corpus because the
            // hyphenated identifiers ("shared-py", "retriever.rs") tokenize
            // poorly and BM25 matches surface-noise functions like
            // `matches_file` and `is_lib_rs` instead of the target. Stick
            // with body-vec-only sub-searches; revisit if a larger embedder
            // (BGE-base or jina-code) replaces BGE-small.
            let original_hits = fetch_code_arm(
                store,
                query,
                query_embedding,
                /* limit */ 12,
                false,
                false,
            )
            .await?;
            // Vote-based project detection across the top-5 of the original
            // results: take the project that appears most often, not just
            // top-1. The query can match a comparison-shaped function in an
            // unrelated project (e.g. `pre_classify_comparison` in code-rag
            // for any query containing "differ"); top-1 alone is too brittle.
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

            // 2. Per-comparator: embed sub-query, search, project-filter, truncate.
            let mut lists: Vec<Vec<(code_rag_types::CodeChunk, f32)>> =
                Vec::with_capacity(comparators.len() + 1);
            for comp in &comparators {
                // Concatenated augmentation: `{comparator} {original query}`.
                // SOTA papers (LlamaIndex SubQuestion, LocAgent) recommend
                // bare-entity sub-queries, but those papers assume large-
                // capacity embedders. BGE-small-en-v1.5 (384d) embeds single
                // hyphenated tokens like "shared-py" poorly — we measured a
                // ~10pp recall drop with bare comparators on this dataset.
                // Concatenation gives the embedder a full sentence to work
                // with while still steering it toward the comparator via the
                // leading token. BM25 (when enabled below) handles the
                // literal-token side independently.
                let sub_query = format!("{} {}", comp, query);
                let emb = embedder.embed_one(&sub_query)?;
                let raw =
                    fetch_code_arm(store, &sub_query, &emb, /* limit */ 12, false, false).await?;
                let filtered: Vec<_> = raw
                    .into_iter()
                    .filter(|(c, _)| target_project.as_ref().is_none_or(|p| &c.project_name == p))
                    .take(8)
                    .collect();
                lists.push(filtered);
            }
            // Push the original-query hits as one more list — robust against
            // a single comparator failing to extract cleanly.
            let original_filtered: Vec<_> = original_hits
                .into_iter()
                .filter(|(c, _)| target_project.as_ref().is_none_or(|p| &c.project_name == p))
                .take(8)
                .collect();
            lists.push(original_filtered);

            // 3. Build a max-score lookup from the union of all per-list
            //    natural relevance scores (each fetch_code_arm returns
            //    `1/(1+dist)`-style values in roughly 0.4–0.7). RRF fuses
            //    *ranks* across lists to decide who gets in; we then assign
            //    each surviving chunk its best natural score so the global
            //    flatten/sort across code+readme+crate+module_doc remains
            //    score-comparable. Raw RRF values (~0.02–0.05) would otherwise
            //    sink the code chunks below every non-code chunk; rescaling
            //    them to a synthetic 0.7–1.0 range made code dominate and
            //    suppressed expected crate/readme hits. Max-of-natural keeps
            //    both signals honest.
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
            let fused = fuse_comparator_lists(lists, config.code_limit);
            let rescored: Vec<(code_rag_types::CodeChunk, f32)> = fused
                .into_iter()
                .map(|(c, _rrf)| {
                    let s = max_score.get(&c.chunk_id).copied().unwrap_or(0.5);
                    (c, s)
                })
                .collect();
            tracing::info!(
                comparators = ?comparators,
                target_project = ?target_project,
                fused_count = rescored.len(),
                "c3: comparison decomposition active"
            );
            rescored
        } else {
            // Extraction failed — fall through to the standard single-arm path.
            fetch_code_arm(
                store,
                query,
                query_embedding,
                fetch_config.code_limit,
                use_hybrid,
                use_sig_vec,
            )
            .await?
        }
    } else {
        fetch_code_arm(
            store,
            query,
            query_embedding,
            fetch_config.code_limit,
            use_hybrid,
            use_sig_vec,
        )
        .await?
    };
    let code_scored = to_scored_relevance(code_raw);

    // C1: Graph augmentation for Relationship intent. Also fires on Implementation
    // because intent classifier has 44% accuracy on Relationship — most relationship
    // queries arrive misclassified as Implementation. graph_augment returns None
    // quickly when no target term is found or no edges match.
    // C2: augment_with_graph now returns (merged_chunks, graph_ids). graph_ids
    // tracks every structurally-confirmed chunk_id (including vector/graph
    // collisions) and is threaded to reserve_graph_slots post-rerank.
    let (code_scored, graph_ids) =
        if intent == QueryIntent::Relationship || intent == QueryIntent::Implementation {
            augment_with_graph(query, code_scored, store).await
        } else {
            (code_scored, std::collections::HashSet::new())
        };

    // Non-code tables are untouched by B5 — they follow the hybrid toggle only.
    let (readme_scored, crate_scored, module_doc_scored) = if use_hybrid {
        let (readme_raw, crate_raw, module_doc_raw) = tokio::try_join!(
            store.hybrid_search_readme(query, query_embedding, fetch_config.readme_limit),
            store.hybrid_search_crates(query, query_embedding, fetch_config.crate_limit),
            store.hybrid_search_module_docs(query, query_embedding, fetch_config.module_doc_limit),
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

    // C2: SOTA routing for structural queries. When the query has explicit
    // direction keywords ("what calls X", "called by", "depends on", etc.),
    // partition graph-confirmed chunks OUT of the rerank pipeline entirely.
    // They carry tier-score confidence (0.75-0.85) representing an actual
    // AST call edge — the cross-encoder (ms-marco-MiniLM) has no way to
    // "see" that and routinely demotes structural hits in favor of textually
    // similar but structurally wrong chunks. Cody / LocAgent / GraphCoder
    // all handle this by routing structural queries to the graph track
    // separately from the semantic reranker. See arXiv:2509.05980 (GRACE)
    // for the formal version; we use slot reservation because we can't
    // retrain the browser-compatible reranker.
    //
    // For ambiguous queries (direction = Both, e.g. "How does X work?"),
    // we keep graph chunks in the rerank pool but over-retain so
    // `reserve_graph_slots` has a buffer to rescue demoted chunks with soft
    // min_slots (1 for Implementation, 2 for Relationship).
    let direction = graph::detect_direction(query);
    let explicit_structural = !graph_ids.is_empty() && direction != graph::GraphDirection::Both;

    // Split graph chunks out before rerank for explicit-direction queries.
    // `graph_reserved` is pre-sorted by tier score (highest confidence first)
    // and capped at (code_limit - 1) to leave at least one slot for the
    // top semantic match (typically the function being asked about).
    //
    // Routing only applies when rerank is active — without rerank there's
    // no demotion to protect against, and the no-rerank and rerank-fallback
    // paths need code_scored intact.
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
        let max_graph_slots = config.code_limit.saturating_sub(1);
        g.truncate(max_graph_slots);
        (g, c)
    } else {
        (Vec::new(), code_scored.clone())
    };

    // Ambiguous-direction soft reserve: over-retain so reserve_graph_slots
    // has a below-cutoff buffer to rescue demoted chunks from.
    let soft_reserve_active = !explicit_structural
        && !graph_ids.is_empty()
        && (intent == QueryIntent::Relationship || intent == QueryIntent::Implementation);
    let code_keep_override = if soft_reserve_active {
        Some(config.code_limit + 5)
    } else {
        None
    };

    // Rerank only the non-graph portion when routing is active. The
    // `graph_reserved` list is stitched back in after rerank.
    let code_for_pipeline = code_for_rerank;
    let mut result = if should_rerank {
        if let Some(reranker) = reranker {
            // Attempt reranking; on failure, fall back to current scores
            let bundle = RetrievalResult {
                code_chunks: code_for_pipeline,
                readme_chunks: readme_scored,
                crate_chunks: crate_scored,
                module_doc_chunks: module_doc_scored,
                intent,
            };
            match rerank_all(query, bundle, reranker, config, code_keep_override) {
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
                            store.hybrid_search_readme(query, query_embedding, config.readme_limit),
                            store.hybrid_search_crates(query, query_embedding, config.crate_limit),
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

    // C2: stitch graph_reserved back into the result.
    //
    // Explicit-structural path (SOTA routing, matches Cody/LocAgent):
    //   Prepend graph_reserved (which was held out of rerank entirely) in
    //   front of the reranked non-graph chunks. Graph chunks keep their
    //   tier_score, reranked chunks keep their cross-encoder score, truncate
    //   to code_limit. The reranker never had authority over structural
    //   hits, so they cannot be demoted.
    //
    // Soft-reserve path (ambiguous direction, e.g. "How does X work?"):
    //   Graph chunks went through rerank with over-retention; use
    //   reserve_graph_slots to swap demoted graph chunks back up from the
    //   below-cutoff buffer. min_slots strength depends on classified intent.
    if explicit_structural {
        let semantic_slots = config.code_limit.saturating_sub(graph_reserved.len());
        let mut combined: Vec<ScoredChunk<code_rag_types::CodeChunk>> = graph_reserved;
        combined.extend(
            std::mem::take(&mut result.code_chunks)
                .into_iter()
                .take(semantic_slots),
        );
        result.code_chunks = combined;
    } else if soft_reserve_active {
        let min_slots = if intent == QueryIntent::Relationship {
            2
        } else {
            1
        };
        result.code_chunks = graph::reserve_graph_slots(
            std::mem::take(&mut result.code_chunks),
            &graph_ids,
            config.code_limit,
            min_slots,
        );
    }

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
