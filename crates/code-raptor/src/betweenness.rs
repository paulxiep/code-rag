//! R4: Brandes edge-betweenness — cross-module bridge detection.
//!
//! Pure (no I/O): scores every edge of the undirected topology by how many
//! shortest paths run through it (Brandes 2001, with the Girvan–Newman edge
//! accumulation). Edges with high betweenness whose endpoints sit in
//! *different* communities are the hidden coupling the architecture report
//! surfaces.
//!
//! **Unweighted BFS.** Edge weight in the topology is a *strength* (summed
//! multi-edges), not a distance, so shortest paths are hop-counted; the weight
//! feeds the surprise score in `analytics` instead.
//!
//! **Source cap.** Brandes from K sources is O(K·E). Above
//! [`BETWEENNESS_SOURCE_CAP`] nodes we run a deterministic stride sample of
//! sources (node indexing is already sorted by chunk id, so the sample is
//! stable across runs). Sampling scales all scores uniformly — the *ranking*
//! the report needs is preserved.

use std::collections::HashMap;

/// Above this many nodes, betweenness runs from a stride-sampled subset of
/// sources instead of all of them (same const pattern as `cluster.rs`).
const BETWEENNESS_SOURCE_CAP: usize = 1500;

use crate::louvain::WeightedGraph;

/// Edge betweenness keyed by `(min(u, v), max(u, v))`. Self-loops are ignored.
/// When all sources run, values match the classic undirected definition (each
/// unordered pair counted once).
pub fn edge_betweenness(g: &WeightedGraph) -> HashMap<(usize, usize), f64> {
    brandes(g, &sample_sources(g.node_count(), BETWEENNESS_SOURCE_CAP))
}

/// Deterministic source selection: all of `0..n` if within the cap, otherwise
/// every `ceil(n / cap)`-th node of the (sorted) index range.
fn sample_sources(n: usize, cap: usize) -> Vec<usize> {
    if n <= cap {
        return (0..n).collect();
    }
    let stride = n.div_ceil(cap);
    (0..n).step_by(stride).collect()
}

fn brandes(g: &WeightedGraph, sources: &[usize]) -> HashMap<(usize, usize), f64> {
    let n = g.node_count();
    let mut score: HashMap<(usize, usize), f64> = HashMap::new();

    // Per-source scratch, reset by stamping visited distances.
    let mut dist: Vec<i64> = vec![-1; n];
    let mut sigma: Vec<f64> = vec![0.0; n];
    let mut delta: Vec<f64> = vec![0.0; n];
    let mut preds: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut order: Vec<usize> = Vec::with_capacity(n);
    let mut queue: std::collections::VecDeque<usize> = std::collections::VecDeque::new();

    for &s in sources {
        for i in 0..n {
            dist[i] = -1;
            sigma[i] = 0.0;
            delta[i] = 0.0;
            preds[i].clear();
        }
        order.clear();
        queue.clear();

        dist[s] = 0;
        sigma[s] = 1.0;
        queue.push_back(s);
        while let Some(v) = queue.pop_front() {
            order.push(v);
            for &(w, _) in g.neighbors(v) {
                if w == v {
                    continue; // self-loop
                }
                if dist[w] < 0 {
                    dist[w] = dist[v] + 1;
                    queue.push_back(w);
                }
                if dist[w] == dist[v] + 1 {
                    sigma[w] += sigma[v];
                    preds[w].push(v);
                }
            }
        }

        // Dependency accumulation onto edges, reverse BFS order.
        for &v in order.iter().rev() {
            for &u in &preds[v] {
                let c = sigma[u] / sigma[v] * (1.0 + delta[v]);
                let key = (u.min(v), u.max(v));
                *score.entry(key).or_insert(0.0) += c;
                delta[u] += c;
            }
        }
    }

    // Each unordered (s, t) pair is seen from both endpoints when all sources
    // run; halve to the classic definition (uniform, so ranking-safe under
    // sampling too).
    for v in score.values_mut() {
        *v /= 2.0;
    }
    score
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_graph_hand_checked() {
        // 0-1-2-3: pairs crossing (0,1)=3, (1,2)=4, (2,3)=3.
        let g = WeightedGraph::from_edges(4, [(0, 1, 1.0), (1, 2, 1.0), (2, 3, 1.0)]);
        let bc = edge_betweenness(&g);
        assert_eq!(bc[&(0, 1)], 3.0);
        assert_eq!(bc[&(1, 2)], 4.0);
        assert_eq!(bc[&(2, 3)], 3.0);
    }

    #[test]
    fn bridge_edge_has_strict_max() {
        // Two triangles {0,1,2} and {3,4,5} joined by bridge 2-3 (the louvain
        // fixture). Every cross-triangle pair crosses the bridge → 9.
        let g = WeightedGraph::from_edges(
            6,
            [
                (0, 1, 1.0),
                (1, 2, 1.0),
                (0, 2, 1.0),
                (3, 4, 1.0),
                (4, 5, 1.0),
                (3, 5, 1.0),
                (2, 3, 1.0),
            ],
        );
        let bc = edge_betweenness(&g);
        let bridge = bc[&(2, 3)];
        assert_eq!(bridge, 9.0);
        for (&edge, &v) in &bc {
            if edge != (2, 3) {
                assert!(v < bridge, "edge {edge:?} ({v}) >= bridge ({bridge})");
            }
        }
    }

    #[test]
    fn deterministic_across_runs() {
        let g = WeightedGraph::from_edges(
            5,
            [(0, 1, 1.0), (1, 2, 2.0), (2, 3, 1.0), (3, 4, 1.0), (0, 4, 1.0)],
        );
        assert_eq!(edge_betweenness(&g), edge_betweenness(&g));
    }

    #[test]
    fn capped_sources_still_rank_bridge_first() {
        let g = WeightedGraph::from_edges(
            6,
            [
                (0, 1, 1.0),
                (1, 2, 1.0),
                (0, 2, 1.0),
                (3, 4, 1.0),
                (4, 5, 1.0),
                (3, 5, 1.0),
                (2, 3, 1.0),
            ],
        );
        // Cap of 3 → stride 2 → sources {0, 2, 4}.
        let sources = sample_sources(6, 3);
        assert_eq!(sources, vec![0, 2, 4]);
        let bc = brandes(&g, &sources);
        let bridge = bc[&(2, 3)];
        for (&edge, &v) in &bc {
            if edge != (2, 3) {
                assert!(v < bridge);
            }
        }
    }

    #[test]
    fn self_loops_ignored() {
        let g = WeightedGraph::from_edges(2, [(0, 0, 5.0), (0, 1, 1.0)]);
        let bc = edge_betweenness(&g);
        assert!(!bc.contains_key(&(0, 0)));
        assert_eq!(bc[&(0, 1)], 1.0);
    }
}
