//! Browser-side BM25 search and RRF fusion for hybrid retrieval (B2).

use serde::Deserialize;
use std::collections::HashMap;

// B5: RRF fusion lives in the shared engine crate so server and browser
// stay in sync. Callers keep using `text_search::rrf_fuse`.
pub use code_rag_engine::fusion::rrf_fuse;

/// Pre-computed IDF table for BM25 scoring.
/// Built during `code-raptor export`, included in the JSON bundle.
#[derive(Debug, Clone, Deserialize)]
pub struct IdfTable {
    pub num_docs: usize,
    pub doc_frequencies: HashMap<String, usize>,
}

impl IdfTable {
    /// IDF score for a term: ln(1 + (N - n + 0.5) / (n + 0.5)) (BM25 variant)
    pub fn idf(&self, term: &str) -> f32 {
        let n = *self.doc_frequencies.get(term).unwrap_or(&0) as f32;
        let big_n = self.num_docs as f32;
        ((big_n - n + 0.5) / (n + 0.5) + 1.0).ln()
    }
}

/// Tokenize identically to server-side `simple` tokenizer:
/// split on non-alphanumeric boundaries, lowercase.
pub fn tokenize(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

/// BM25 search for in-browser hybrid search.
/// Uses pre-computed IDF from the exported JSON bundle.
pub fn bm25_search<T: Clone>(
    query: &str,
    chunks: &[super::data::EmbeddedChunk<T>],
    text_fn: impl Fn(&T) -> &str,
    idf_table: &IdfTable,
    limit: usize,
) -> Vec<(T, f32)> {
    let query_tokens = tokenize(query);
    if query_tokens.is_empty() {
        return vec![];
    }
    let k1 = 1.2_f32;
    let b = 0.75_f32;

    // Compute average document length
    let avg_dl: f32 = if chunks.is_empty() {
        1.0
    } else {
        chunks
            .iter()
            .map(|ec| tokenize(text_fn(&ec.chunk)).len() as f32)
            .sum::<f32>()
            / chunks.len() as f32
    };

    let mut scored: Vec<(T, f32)> = chunks
        .iter()
        .map(|ec| {
            let text = text_fn(&ec.chunk);
            let doc_tokens = tokenize(text);
            let dl = doc_tokens.len() as f32;

            let score: f32 = query_tokens
                .iter()
                .map(|qt| {
                    let tf = doc_tokens.iter().filter(|t| *t == qt).count() as f32;
                    if tf == 0.0 {
                        return 0.0;
                    }
                    let idf = idf_table.idf(qt);
                    idf * (tf * (k1 + 1.0)) / (tf + k1 * (1.0 - b + b * dl / avg_dl))
                })
                .sum();

            (ec.chunk.clone(), score)
        })
        .filter(|(_, score)| *score > 0.0)
        .collect();

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(limit);
    scored
}

/// BM25 search using pre-computed text strings (B3).
/// Like bm25_search but uses a Vec<String> instead of a closure for text extraction.
/// Used for code chunks where searchable_text is computed at load time.
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
    let k1 = 1.2_f32;
    let b = 0.75_f32;

    let avg_dl: f32 =
        texts.iter().map(|t| tokenize(t).len() as f32).sum::<f32>() / texts.len() as f32;

    let mut scored: Vec<(T, f32)> = chunks
        .iter()
        .zip(texts.iter())
        .map(|(ec, text)| {
            let doc_tokens = tokenize(text);
            let dl = doc_tokens.len() as f32;

            let score: f32 = query_tokens
                .iter()
                .map(|qt| {
                    let tf = doc_tokens.iter().filter(|t| *t == qt).count() as f32;
                    if tf == 0.0 {
                        return 0.0;
                    }
                    let idf = idf_table.idf(qt);
                    idf * (tf * (k1 + 1.0)) / (tf + k1 * (1.0 - b + b * dl / avg_dl))
                })
                .sum();

            (ec.chunk.clone(), score)
        })
        .filter(|(_, score)| *score > 0.0)
        .collect();

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(limit);
    scored
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_idf_table() -> IdfTable {
        let mut doc_frequencies = HashMap::new();
        doc_frequencies.insert("fn".to_string(), 50); // very common
        doc_frequencies.insert("retrieve".to_string(), 2); // rare
        doc_frequencies.insert("pub".to_string(), 45); // very common
        doc_frequencies.insert("search".to_string(), 5); // moderate
        IdfTable {
            num_docs: 100,
            doc_frequencies,
        }
    }

    #[test]
    fn test_tokenize_snake_case() {
        let tokens = tokenize("distance_to_relevance");
        assert_eq!(tokens, vec!["distance", "to", "relevance"]);
    }

    #[test]
    fn test_tokenize_case_insensitive() {
        let tokens = tokenize("VectorStore");
        assert_eq!(tokens, vec!["vectorstore"]);
    }

    #[test]
    fn test_idf_rare_vs_common() {
        let idf = make_idf_table();
        let rare_idf = idf.idf("retrieve");
        let common_idf = idf.idf("fn");
        assert!(rare_idf > common_idf, "Rare term should have higher IDF");
    }

    #[test]
    fn test_idf_unknown_term() {
        let idf = make_idf_table();
        let unknown_idf = idf.idf("nonexistent");
        assert!(unknown_idf > 0.0, "Unknown term should have positive IDF");
    }

    // rrf_fuse tests moved to code-rag-engine::fusion — this crate just re-exports it.
}
