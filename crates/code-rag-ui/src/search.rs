//! Brute-force vector search over in-memory chunks.

use code_rag_engine::config::RetrievalConfig;

use crate::data::{ChunkIndex, EmbeddedChunk};

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

/// Search all chunk types and return raw (chunk, distance) tuples.
/// Caller uses `code_rag_engine::retriever::to_retrieval_result` to convert.
pub fn brute_force_search(
    query_embedding: &[f32],
    index: &ChunkIndex,
    config: &RetrievalConfig,
) -> (
    Vec<(code_rag_types::CodeChunk, f32)>,
    Vec<(code_rag_types::ReadmeChunk, f32)>,
    Vec<(code_rag_types::CrateChunk, f32)>,
    Vec<(code_rag_types::ModuleDocChunk, f32)>,
) {
    (
        top_k(query_embedding, &index.code_chunks, config.code_limit),
        top_k(query_embedding, &index.readme_chunks, config.readme_limit),
        top_k(query_embedding, &index.crate_chunks, config.crate_limit),
        top_k(
            query_embedding,
            &index.module_doc_chunks,
            config.module_doc_limit,
        ),
    )
}

/// Hybrid search: vector + BM25 combined via RRF fusion.
/// Returns (chunk, rrf_score) tuples where score is higher=better.
/// Falls back to vector-only if IDF tables are not available.
pub fn hybrid_search(
    query: &str,
    query_embedding: &[f32],
    index: &ChunkIndex,
    config: &RetrievalConfig,
) -> (
    Vec<(code_rag_types::CodeChunk, f32)>,
    Vec<(code_rag_types::ReadmeChunk, f32)>,
    Vec<(code_rag_types::CrateChunk, f32)>,
    Vec<(code_rag_types::ModuleDocChunk, f32)>,
) {
    use crate::text_search::{bm25_search, rrf_fuse};

    let code = if let Some(ref idf) = index.code_idf {
        let vec_results = top_k(query_embedding, &index.code_chunks, config.code_limit);
        let bm25_results = bm25_search(
            query,
            &index.code_chunks,
            |c| &c.code_content,
            idf,
            config.code_limit,
        );
        rrf_fuse(vec_results, bm25_results, 60, |c| &c.chunk_id)
    } else {
        top_k(query_embedding, &index.code_chunks, config.code_limit)
    };

    let readme = if let Some(ref idf) = index.readme_idf {
        let vec_results = top_k(query_embedding, &index.readme_chunks, config.readme_limit);
        let bm25_results = bm25_search(
            query,
            &index.readme_chunks,
            |c| &c.content,
            idf,
            config.readme_limit,
        );
        rrf_fuse(vec_results, bm25_results, 60, |c| &c.chunk_id)
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
        rrf_fuse(vec_results, bm25_results, 60, |c| &c.chunk_id)
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
        rrf_fuse(vec_results, bm25_results, 60, |c| &c.chunk_id)
    } else {
        top_k(
            query_embedding,
            &index.module_doc_chunks,
            config.module_doc_limit,
        )
    };

    (code, readme, crates, module_docs)
}
