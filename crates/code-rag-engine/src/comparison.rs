//! C3: Comparison query decomposition — pure RRF fusion of pre-fetched
//! per-comparator candidate lists.
//!
//! Callers (server `retrieve()`, WASM `run_retrieval`) pre-fetch one candidate
//! list per comparator (plus the original-query list) and pass them in. This
//! helper is sync, has no I/O, and is wasm-safe.

use crate::fusion::rrf_fuse;
use code_rag_types::CodeChunk;

/// Fuse N pre-fetched comparator candidate lists via RRF and truncate to
/// `final_limit`.
///
/// `rrf_fuse` ignores raw score magnitudes and uses only rank within each
/// list, so it doesn't matter whether the input scores are L2 distances or
/// relevance values. The output scores are RRF (higher = better).
pub fn fuse_comparator_lists(
    lists: Vec<Vec<(CodeChunk, f32)>>,
    final_limit: usize,
) -> Vec<(CodeChunk, f32)> {
    if lists.is_empty() {
        return Vec::new();
    }
    let mut fused = rrf_fuse(&lists, 60, |c: &CodeChunk| c.chunk_id.as_str());
    fused.truncate(final_limit);
    fused
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk(id: &str) -> CodeChunk {
        CodeChunk {
            file_path: format!("{id}.rs"),
            language: "rust".to_string(),
            identifier: id.to_string(),
            node_type: "function_definition".to_string(),
            code_content: String::new(),
            start_line: 0,
            project_name: "test".to_string(),
            docstring: None,
            signature: None,
            chunk_id: id.to_string(),
            content_hash: String::new(),
            embedding_model_version: String::new(),
        }
    }

    #[test]
    fn empty_input_yields_empty_output() {
        let out = fuse_comparator_lists(Vec::new(), 5);
        assert!(out.is_empty());
    }

    #[test]
    fn single_list_passes_through_truncated() {
        let lists = vec![vec![
            (chunk("a"), 0.9),
            (chunk("b"), 0.8),
            (chunk("c"), 0.7),
        ]];
        let out = fuse_comparator_lists(lists, 2);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].0.chunk_id, "a");
        assert_eq!(out[1].0.chunk_id, "b");
    }

    #[test]
    fn fuses_disjoint_lists_via_rrf() {
        // Two lists with no overlap — RRF should interleave by rank.
        let lists = vec![
            vec![(chunk("a"), 0.9), (chunk("b"), 0.8)],
            vec![(chunk("x"), 0.9), (chunk("y"), 0.8)],
        ];
        let out = fuse_comparator_lists(lists, 4);
        assert_eq!(out.len(), 4);
        let ids: Vec<_> = out.iter().map(|(c, _)| c.chunk_id.as_str()).collect();
        // Top-1 from each list should both appear in the top-2.
        assert!(ids.contains(&"a"));
        assert!(ids.contains(&"x"));
    }

    #[test]
    fn overlapping_lists_get_boosted() {
        // "a" appears in both lists at rank 1 — should fuse to the top.
        let lists = vec![
            vec![(chunk("a"), 0.9), (chunk("b"), 0.8)],
            vec![(chunk("a"), 0.9), (chunk("c"), 0.8)],
        ];
        let out = fuse_comparator_lists(lists, 5);
        assert_eq!(out[0].0.chunk_id, "a");
        // Output should dedupe — "a" appears once.
        assert_eq!(out.iter().filter(|(c, _)| c.chunk_id == "a").count(), 1);
    }
}
