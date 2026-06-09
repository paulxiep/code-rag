//! Deterministic Louvain community detection (Track R, R2).
//!
//! Modularity-maximizing community detection over a weighted, undirected graph
//! whose nodes are contiguous integers `0..n`. This is a from-scratch native
//! implementation: Rust has no graspologic/Leiden equivalent and `petgraph`
//! ships no community detection, so we own the algorithm.
//!
//! **Leiden is deferred.** R.md / the development plan specify "Leiden (Louvain
//! fallback)"; R2 ships Louvain only. Leiden's sole added guarantee
//! (well-connected communities — no internally-disconnected community survives
//! a level) is needed only if a spot-check / the R4 report shows a disconnected
//! community. The revisit is a refinement phase layered over each Louvain
//! community here; see `development_plan.md` "Leiden deferred".
//!
//! **Determinism.** Classic Louvain randomizes node visit order each pass; we
//! instead visit nodes in fixed ascending index order, break modularity-gain
//! ties toward the smallest community index, and iterate neighbor communities
//! in sorted order. Identical input therefore yields identical labels across
//! runs — no RNG, no seed needed.

use std::collections::HashMap;

const EPS: f64 = 1e-12;

/// Weighted undirected graph over contiguous node ids `0..n`.
///
/// Each non-self edge `{u, v}` is stored in both `adjacency[u]` and
/// `adjacency[v]`; a self-loop `{i, i}` is stored once in `adjacency[i]`.
/// Multi-edges between the same pair are summed at build time.
#[derive(Clone, Debug)]
pub struct WeightedGraph {
    n: usize,
    /// Sorted-by-neighbor `(neighbor, weight)` adjacency.
    adjacency: Vec<Vec<(usize, f64)>>,
    /// Weighted degree of each node (self-loop counted twice).
    degree: Vec<f64>,
    /// Total edge weight `m` (each undirected edge counted once).
    total_weight: f64,
}

impl WeightedGraph {
    /// Build from `n` nodes and undirected weighted edges. `u == v` is a
    /// self-loop (used by aggregation); duplicate pairs are summed.
    pub fn from_edges(n: usize, edges: impl IntoIterator<Item = (usize, usize, f64)>) -> Self {
        let mut maps: Vec<HashMap<usize, f64>> = vec![HashMap::new(); n];
        for (u, v, w) in edges {
            debug_assert!(u < n && v < n);
            if u == v {
                *maps[u].entry(u).or_insert(0.0) += w;
            } else {
                *maps[u].entry(v).or_insert(0.0) += w;
                *maps[v].entry(u).or_insert(0.0) += w;
            }
        }
        let mut adjacency = Vec::with_capacity(n);
        let mut degree = vec![0.0; n];
        for (i, map) in maps.into_iter().enumerate() {
            let mut neigh: Vec<(usize, f64)> = map.into_iter().collect();
            neigh.sort_by_key(|&(j, _)| j);
            let mut d = 0.0;
            for &(j, w) in &neigh {
                d += w;
                if j == i {
                    // self-loop contributes to degree twice
                    d += w;
                }
            }
            degree[i] = d;
            adjacency.push(neigh);
        }
        let total_weight = degree.iter().sum::<f64>() / 2.0;
        WeightedGraph {
            n,
            adjacency,
            degree,
            total_weight,
        }
    }

    pub fn node_count(&self) -> usize {
        self.n
    }

    pub fn neighbors(&self, i: usize) -> &[(usize, f64)] {
        &self.adjacency[i]
    }

    /// Weighted degree of node `i` (self-loop counted twice).
    pub fn degree(&self, i: usize) -> f64 {
        self.degree[i]
    }

    /// Number of distinct undirected edges among `members` (self-loops ignored).
    /// Used for cohesion scoring.
    pub fn intra_edge_count(&self, members: &std::collections::HashSet<usize>) -> usize {
        let mut count = 0usize;
        for &i in members {
            for &(j, _) in &self.adjacency[i] {
                // count each undirected pair once; skip self-loops
                if j > i && members.contains(&j) {
                    count += 1;
                }
            }
        }
        count
    }
}

/// Run multi-level Louvain. Returns a community label in `0..k` for every node
/// (contiguous, but otherwise arbitrary — callers re-index for stable ids).
pub fn louvain(graph: &WeightedGraph) -> Vec<usize> {
    let n = graph.node_count();
    if n == 0 {
        return Vec::new();
    }
    let mut result: Vec<usize> = (0..n).collect();
    let mut working = graph.clone();
    loop {
        let (comm, moved) = one_level(&working);
        if !moved {
            break;
        }
        for r in result.iter_mut() {
            *r = comm[*r];
        }
        let n_comm = comm.iter().map(|&c| c + 1).max().unwrap_or(0);
        if n_comm == working.node_count() {
            // No merges possible — fixed point reached.
            break;
        }
        working = aggregate(&working, &comm, n_comm);
    }
    result
}

/// One level of local moving. Returns `(community_per_node, moved_any)` with
/// communities renumbered contiguously by ascending original label.
fn one_level(g: &WeightedGraph) -> (Vec<usize>, bool) {
    let n = g.node_count();
    let m = g.total_weight;
    if m <= 0.0 {
        return ((0..n).collect(), false);
    }
    let two_m = 2.0 * m;

    let mut comm: Vec<usize> = (0..n).collect();
    // Sum of degrees of nodes currently in each community.
    let mut tot: Vec<f64> = g.degree.clone();

    let mut moved_any = false;
    loop {
        let mut moved = false;
        for i in 0..n {
            let ci = comm[i];
            let ki = g.degree[i];

            // Weight from i to each neighboring community (sorted for determinism).
            let mut neigh_w: HashMap<usize, f64> = HashMap::new();
            for &(j, w) in &g.adjacency[i] {
                if j == i {
                    continue;
                }
                *neigh_w.entry(comm[j]).or_insert(0.0) += w;
            }

            // Tentatively remove i from its community.
            tot[ci] -= ki;

            // Baseline: returning to ci.
            let w_to_ci = neigh_w.get(&ci).copied().unwrap_or(0.0);
            let mut best_c = ci;
            let mut best_gain = w_to_ci - tot[ci] * ki / two_m;

            let mut candidates: Vec<(usize, f64)> = neigh_w.into_iter().collect();
            candidates.sort_by_key(|&(c, _)| c);
            for (c, w) in candidates {
                let gain = w - tot[c] * ki / two_m;
                if gain > best_gain + EPS || ((gain - best_gain).abs() <= EPS && c < best_c) {
                    best_gain = gain;
                    best_c = c;
                }
            }

            tot[best_c] += ki;
            comm[i] = best_c;
            if best_c != ci {
                moved = true;
                moved_any = true;
            }
        }
        if !moved {
            break;
        }
    }

    renumber(&mut comm);
    (comm, moved_any)
}

/// Renumber community labels to a contiguous `0..k` range, ordered by ascending
/// original label so the mapping is deterministic.
fn renumber(comm: &mut [usize]) {
    let mut labels: Vec<usize> = comm.to_vec();
    labels.sort_unstable();
    labels.dedup();
    let remap: HashMap<usize, usize> = labels.iter().enumerate().map(|(new, &old)| (old, new)).collect();
    for c in comm.iter_mut() {
        *c = remap[c];
    }
}

/// Collapse each community into a super-node, summing edge weights. Intra-community
/// weight becomes the super-node's self-loop.
fn aggregate(g: &WeightedGraph, comm: &[usize], n_comm: usize) -> WeightedGraph {
    let mut edges: Vec<(usize, usize, f64)> = Vec::new();
    for i in 0..g.node_count() {
        for &(j, w) in &g.adjacency[i] {
            if j < i {
                continue; // each undirected pair once
            }
            let (cu, cv) = (comm[i], comm[j]);
            // j == i (self-loop) or same community → self-loop on the super-node.
            edges.push((cu, cv, w));
        }
    }
    WeightedGraph::from_edges(n_comm, edges)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn empty_graph() {
        let g = WeightedGraph::from_edges(0, []);
        assert!(louvain(&g).is_empty());
    }

    #[test]
    fn single_node() {
        let g = WeightedGraph::from_edges(1, []);
        assert_eq!(louvain(&g), vec![0]);
    }

    #[test]
    fn two_triangles_one_bridge() {
        // Triangle {0,1,2}, triangle {3,4,5}, single bridge edge 2-3.
        let edges = [
            (0, 1, 1.0),
            (1, 2, 1.0),
            (0, 2, 1.0),
            (3, 4, 1.0),
            (4, 5, 1.0),
            (3, 5, 1.0),
            (2, 3, 1.0),
        ];
        let g = WeightedGraph::from_edges(6, edges);
        let comm = louvain(&g);
        // Two communities: {0,1,2} and {3,4,5}.
        assert_eq!(comm[0], comm[1]);
        assert_eq!(comm[1], comm[2]);
        assert_eq!(comm[3], comm[4]);
        assert_eq!(comm[4], comm[5]);
        assert_ne!(comm[0], comm[3]);
    }

    #[test]
    fn deterministic_across_runs() {
        let edges = [
            (0, 1, 1.0),
            (1, 2, 1.0),
            (0, 2, 1.0),
            (3, 4, 1.0),
            (4, 5, 1.0),
            (3, 5, 1.0),
            (2, 3, 1.0),
            (5, 6, 1.0),
            (6, 7, 1.0),
            (7, 8, 1.0),
            (6, 8, 1.0),
        ];
        let g = WeightedGraph::from_edges(9, edges);
        let a = louvain(&g);
        let b = louvain(&g);
        assert_eq!(a, b);
    }

    #[test]
    fn cohesion_intra_edges() {
        let edges = [(0, 1, 1.0), (1, 2, 1.0), (0, 2, 1.0), (2, 3, 1.0)];
        let g = WeightedGraph::from_edges(4, edges);
        let members: HashSet<usize> = [0, 1, 2].into_iter().collect();
        // {0,1,2} is a triangle → 3 intra edges.
        assert_eq!(g.intra_edge_count(&members), 3);
    }
}
