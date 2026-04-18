//! Browser-side BM25 search and RRF fusion for hybrid retrieval (B2).
//!
//! Post-A1 this is a thin adapter over `code_rag_engine::text` — the IDF table,
//! tokenizer, and BM25 kernel all live in the shared engine crate. This module
//! only handles the Vec<T> iteration / filter / sort / truncate around scoring.

// Re-exports so call sites in `search.rs` (and any future browser-side callers)
// can keep importing from this module rather than reaching into the engine.
pub use code_rag_engine::fusion::rrf_fuse;
pub use code_rag_engine::text::IdfTable;

use code_rag_engine::text::{Bm25Params, score, tokenize};

/// BM25 search over a slice of EmbeddedChunks, using a caller-supplied text extractor.
/// Uses pre-computed IDF from the exported JSON bundle.
pub fn bm25_search<T: Clone>(
    query: &str,
    chunks: &[super::data::EmbeddedChunk<T>],
    text_fn: impl Fn(&T) -> &str,
    idf_table: &IdfTable,
    limit: usize,
) -> Vec<(T, f32)> {
    let query_tokens = tokenize(query);
    if query_tokens.is_empty() || chunks.is_empty() {
        return vec![];
    }

    let doc_tokens: Vec<Vec<String>> = chunks
        .iter()
        .map(|ec| tokenize(text_fn(&ec.chunk)))
        .collect();

    let avg_doc_len: f32 =
        doc_tokens.iter().map(|d| d.len() as f32).sum::<f32>() / doc_tokens.len() as f32;

    let params = Bm25Params::default();
    let mut scored: Vec<(T, f32)> = chunks
        .iter()
        .zip(doc_tokens.iter())
        .map(|(ec, doc)| {
            let s = score(&query_tokens, doc, avg_doc_len, idf_table, params);
            (ec.chunk.clone(), s)
        })
        .filter(|(_, s)| *s > 0.0)
        .collect();

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(limit);
    scored
}

/// BM25 search using pre-computed text strings (B3).
/// Like bm25_search but uses a parallel `texts` slice instead of a closure for
/// text extraction. Used for code chunks where searchable_text is computed at
/// load time.
pub fn bm25_search_precomputed<T: Clone>(
    query: &str,
    chunks: &[super::data::EmbeddedChunk<T>],
    texts: &[String],
    idf_table: &IdfTable,
    limit: usize,
) -> Vec<(T, f32)> {
    let query_tokens = tokenize(query);
    if query_tokens.is_empty() || chunks.is_empty() {
        return vec![];
    }

    let doc_tokens: Vec<Vec<String>> = texts.iter().map(|t| tokenize(t)).collect();
    let avg_doc_len: f32 =
        doc_tokens.iter().map(|d| d.len() as f32).sum::<f32>() / doc_tokens.len() as f32;

    let params = Bm25Params::default();
    let mut scored: Vec<(T, f32)> = chunks
        .iter()
        .zip(doc_tokens.iter())
        .map(|(ec, doc)| {
            let s = score(&query_tokens, doc, avg_doc_len, idf_table, params);
            (ec.chunk.clone(), s)
        })
        .filter(|(_, s)| *s > 0.0)
        .collect();

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(limit);
    scored
}
