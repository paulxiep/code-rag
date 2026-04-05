//! Reciprocal Rank Fusion for combining multiple ranked result lists.
//!
//! Generic over chunk type; each arm passes a closure to extract a stable ID.
//! Used by both the server (dual-vector + BM25 fusion) and the browser
//! (vector + BM25 fusion). Pure-Rust, WASM-compatible.

use std::collections::HashMap;

/// Reciprocal Rank Fusion across N ranked lists.
///
/// Each list contributes `1 / (k + rank + 1)` per chunk; scores sum across
/// lists so chunks found by multiple arms rank higher than chunks found by
/// only one. Input scores are ignored — only rank within each list matters.
///
/// `k=60` is the canonical RRF constant from the original paper.
pub fn rrf_fuse<T, F>(lists: &[Vec<(T, f32)>], k: usize, id_fn: F) -> Vec<(T, f32)>
where
    T: Clone,
    F: Fn(&T) -> &str,
{
    let mut scores: HashMap<String, (T, f32)> = HashMap::new();

    for list in lists {
        for (rank, (chunk, _)) in list.iter().enumerate() {
            let id = id_fn(chunk).to_string();
            let rrf_score = 1.0 / (k as f32 + rank as f32 + 1.0);
            scores
                .entry(id)
                .or_insert_with(|| (chunk.clone(), 0.0))
                .1 += rrf_score;
        }
    }

    let mut fused: Vec<(T, f32)> = scores.into_values().collect();
    fused.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    fused
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn test_rrf_fuse_basic() {
        let vec_results = vec![("a", 0.9), ("b", 0.7), ("c", 0.5)];
        let text_results = vec![("b", 5.0), ("c", 3.0), ("d", 1.0)];

        let fused = rrf_fuse(&[vec_results, text_results], 60, |s: &&str| s);
        // "b" appears in both lists → highest RRF score
        assert_eq!(fused[0].0, "b");
    }

    #[test]
    fn test_rrf_fuse_disjoint() {
        let vec_results = vec![("a", 0.9)];
        let text_results = vec![("b", 5.0)];

        let fused = rrf_fuse(&[vec_results, text_results], 60, |s: &&str| s);
        assert_eq!(fused.len(), 2);
        // Same RRF score (both rank 0), order is arbitrary but both present
        let ids: HashSet<&str> = fused.iter().map(|(id, _)| *id).collect();
        assert!(ids.contains("a"));
        assert!(ids.contains("b"));
    }

    #[test]
    fn test_rrf_fuse_three_arms() {
        // Three arms: body-vec, sig-vec, BM25. Chunk "b" appears in all three,
        // "a" in two, "c" in one. "b" should win.
        let body_vec = vec![("a", 0.9), ("b", 0.7), ("c", 0.5)];
        let sig_vec = vec![("b", 0.8), ("a", 0.6)];
        let bm25 = vec![("b", 5.0), ("d", 2.0)];

        let fused = rrf_fuse(&[body_vec, sig_vec, bm25], 60, |s: &&str| s);
        assert_eq!(fused[0].0, "b", "b is in all three arms, should rank first");
        assert_eq!(fused[1].0, "a", "a is in two arms, should rank second");
    }

    #[test]
    fn test_rrf_fuse_single_arm_passthrough_semantics() {
        // Single arm: order preserved; all scores equal 1/(k+rank+1).
        let only = vec![("a", 0.9), ("b", 0.5), ("c", 0.1)];
        let fused = rrf_fuse(&[only], 60, |s: &&str| s);
        assert_eq!(fused.len(), 3);
        assert_eq!(fused[0].0, "a");
        assert_eq!(fused[1].0, "b");
        assert_eq!(fused[2].0, "c");
    }

    #[test]
    fn test_rrf_fuse_empty_lists() {
        let fused: Vec<(&str, f32)> = rrf_fuse::<&str, _>(&[], 60, |s| s);
        assert!(fused.is_empty());
    }
}
