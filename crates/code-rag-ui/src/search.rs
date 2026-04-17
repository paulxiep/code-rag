//! Brute-force vector search over in-memory chunks.

use code_rag_engine::config::RetrievalConfig;

use crate::data::{ChunkIndex, EmbeddedChunk};

/// Bundled search results for the non-code chunk arms (README, crate,
/// module-doc, A2 folder). Each `Vec` is a list of `(chunk, score)` pairs
/// where the score is either an RRF rank (hybrid path) or an L2 distance
/// (brute-force path); the caller decides how to interpret it via the
/// matching `to_scored*` helper.
pub struct NonCodeResults {
    pub readme: Vec<(code_rag_types::ReadmeChunk, f32)>,
    pub crates: Vec<(code_rag_types::CrateChunk, f32)>,
    pub module_docs: Vec<(code_rag_types::ModuleDocChunk, f32)>,
    pub folders: Vec<(code_rag_types::FolderChunk, f32)>,
}

/// Compute L2 (Euclidean) distance between two vectors.
fn l2_distance(a: &[f32], b: &[f32]) -> f32 {
    a.iter()
        .zip(b.iter())
        .map(|(x, y)| (x - y) * (x - y))
        .sum::<f32>()
        .sqrt()
}

/// Find top-k nearest chunks by L2 distance, return (chunk, distance) pairs.
fn top_k<T: Clone>(query: &[f32], chunks: &[EmbeddedChunk<T>], limit: usize) -> Vec<(T, f32)> {
    let mut scored: Vec<(T, f32)> = chunks
        .iter()
        .map(|ec| (ec.chunk.clone(), l2_distance(query, &ec.embedding)))
        .collect();

    scored.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(limit);
    scored
}

/// B5: Find top-k nearest chunks by L2 distance against the signature_embedding.
/// Skips chunks whose `signature_embedding` is None.
fn top_k_signature<T: Clone>(
    query: &[f32],
    chunks: &[EmbeddedChunk<T>],
    limit: usize,
) -> Vec<(T, f32)> {
    let mut scored: Vec<(T, f32)> = chunks
        .iter()
        .filter_map(|ec| {
            ec.signature_embedding
                .as_ref()
                .map(|sig| (ec.chunk.clone(), l2_distance(query, sig)))
        })
        .collect();

    scored.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(limit);
    scored
}

/// Search the CODE arm, respecting hybrid + dual-embedding toggles.
/// Returns (chunk, score) tuples where scores are higher=better when any
/// arm is active, and L2 distances (lower=better) when only body-vec runs.
/// Callers convert via `retriever::to_scored` or `retriever::to_scored_relevance`
/// depending on which branch they took.
pub fn search_code_arm(
    query: &str,
    query_embedding: &[f32],
    index: &ChunkIndex,
    limit: usize,
    use_hybrid: bool,
    use_sig_vec: bool,
) -> (Vec<(code_rag_types::CodeChunk, f32)>, bool) {
    use crate::text_search::{bm25_search_precomputed, rrf_fuse};

    // body_vec_distances carries L2 distances (lower=better). Other arms carry
    // relevance-style scores. rrf_fuse ignores the score magnitudes and only
    // uses rank within each list, so mixing is safe.
    let body_vec = top_k(query_embedding, &index.code_chunks, limit);

    let bm25 = if use_hybrid && index.code_idf.is_some() {
        Some(bm25_search_precomputed(
            query,
            &index.code_chunks,
            &index.code_searchable_texts,
            index.code_idf.as_ref().unwrap(),
            limit,
        ))
    } else {
        None
    };

    let sig_vec = if use_sig_vec {
        Some(top_k_signature(query_embedding, &index.code_chunks, limit))
    } else {
        None
    };

    // Count active fused arms. body_vec is always on; the other two are
    // optional. If only body_vec is active, return distances as-is so the
    // caller can apply the standard distance→relevance conversion.
    let fused_arms: Vec<Vec<(code_rag_types::CodeChunk, f32)>> = std::iter::once(body_vec.clone())
        .chain(bm25)
        .chain(sig_vec)
        .collect();

    if fused_arms.len() == 1 {
        return (body_vec, false); // false = scores are L2 distances
    }

    let fused = rrf_fuse(&fused_arms, 60, |c: &code_rag_types::CodeChunk| {
        c.chunk_id.as_str()
    });
    (fused, true) // true = scores are higher=better RRF scores
}

/// Hybrid search: vector + BM25 combined via RRF fusion.
/// Returns (chunk, rrf_score) tuples where score is higher=better.
/// Falls back to vector-only if IDF tables are not available.
///
/// This helper handles README / Crate / ModuleDoc tables uniformly. The CODE
/// table is handled by `search_code_arm` because it has an extra sig-vec arm.
pub fn hybrid_search_non_code(
    query: &str,
    query_embedding: &[f32],
    index: &ChunkIndex,
    config: &RetrievalConfig,
) -> NonCodeResults {
    use crate::text_search::{bm25_search, rrf_fuse};

    let readme = if let Some(ref idf) = index.readme_idf {
        let vec_results = top_k(query_embedding, &index.readme_chunks, config.readme_limit);
        let bm25_results = bm25_search(
            query,
            &index.readme_chunks,
            |c| &c.content,
            idf,
            config.readme_limit,
        );
        rrf_fuse(&[vec_results, bm25_results], 60, |c| &c.chunk_id)
    } else {
        top_k(query_embedding, &index.readme_chunks, config.readme_limit)
    };

    let crates = if let Some(ref idf) = index.crate_idf {
        let vec_results = top_k(query_embedding, &index.crate_chunks, config.crate_limit);
        let bm25_results = bm25_search(
            query,
            &index.crate_chunks,
            |c| c.description.as_deref().unwrap_or(""),
            idf,
            config.crate_limit,
        );
        rrf_fuse(&[vec_results, bm25_results], 60, |c| &c.chunk_id)
    } else {
        top_k(query_embedding, &index.crate_chunks, config.crate_limit)
    };

    let module_docs = if let Some(ref idf) = index.module_doc_idf {
        let vec_results = top_k(
            query_embedding,
            &index.module_doc_chunks,
            config.module_doc_limit,
        );
        let bm25_results = bm25_search(
            query,
            &index.module_doc_chunks,
            |c| &c.doc_content,
            idf,
            config.module_doc_limit,
        );
        rrf_fuse(&[vec_results, bm25_results], 60, |c| &c.chunk_id)
    } else {
        top_k(
            query_embedding,
            &index.module_doc_chunks,
            config.module_doc_limit,
        )
    };

    // A2: folder arm. Short-circuits when limit == 0 (A2 default), so no
    // work happens until A3 flips folder_limit per intent.
    let folders = if config.folder_limit == 0 {
        Vec::new()
    } else if let Some(ref idf) = index.folder_idf {
        let vec_results = top_k(query_embedding, &index.folder_chunks, config.folder_limit);
        let bm25_results = bm25_search(
            query,
            &index.folder_chunks,
            |c| c.summary_text.as_str(),
            idf,
            config.folder_limit,
        );
        rrf_fuse(&[vec_results, bm25_results], 60, |c| &c.chunk_id)
    } else {
        top_k(query_embedding, &index.folder_chunks, config.folder_limit)
    };

    NonCodeResults {
        readme,
        crates,
        module_docs,
        folders,
    }
}

/// Brute-force vector search for non-code chunk types.
pub fn brute_force_non_code(
    query_embedding: &[f32],
    index: &ChunkIndex,
    config: &RetrievalConfig,
) -> NonCodeResults {
    NonCodeResults {
        readme: top_k(query_embedding, &index.readme_chunks, config.readme_limit),
        crates: top_k(query_embedding, &index.crate_chunks, config.crate_limit),
        module_docs: top_k(
            query_embedding,
            &index.module_doc_chunks,
            config.module_doc_limit,
        ),
        // A2: `top_k` with limit 0 returns empty, so no gate needed here.
        folders: top_k(query_embedding, &index.folder_chunks, config.folder_limit),
    }
}
