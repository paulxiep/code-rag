//! Community refinement + cohesion (Track R, R2).
//!
//! Wraps the deterministic Louvain core with the cross-cutting handling R.md
//! calls for, then emits one community id + cohesion per **code** chunk:
//!
//! 1. **Hub exclusion** — code-node utility super-hubs (degree above a p99/floor
//!    threshold) are pulled out before partitioning and reattached by majority
//!    vote. Container (file) nodes are *never* excluded: they carry the
//!    file-level cohesion + import signal, and the only folder-tree distortion
//!    risk (folders) is already gone — folders aren't nodes at all.
//! 2. **Oversized split** — any community larger than 25% of the graph is
//!    re-partitioned recursively (depth-bounded).
//! 3. **Low-cohesion re-split** — large (≥50-node) communities below cohesion
//!    0.05 are re-partitioned too.
//! 4. **Cohesion** = intra-community edges / max possible, persisted per member.
//! 5. **Stable ids** — surviving communities are re-indexed by `(code-member
//!    count desc, min code chunk_id asc)`, so identical input → identical ids.
//!
//! Everything is deterministic (no RNG): fixed node order + sorted tie-breaks.

use std::collections::{HashMap, HashSet};

use crate::louvain::{WeightedGraph, louvain};
use crate::topology::Topology;

/// Communities larger than this fraction of the graph are re-split.
const OVERSIZE_FRACTION: f64 = 0.25;
/// Low-cohesion re-split only applies to communities at least this large.
const LOW_COHESION_MIN_SIZE: usize = 50;
/// Cohesion below this (for a large community) triggers a re-split.
const LOW_COHESION_THRESHOLD: f64 = 0.05;
/// Code nodes with weighted degree strictly above the hub threshold are excluded
/// from the initial partition. The threshold is `max(p99 degree, this floor)` so
/// small graphs don't over-exclude moderate nodes.
const HUB_DEGREE_FLOOR: f64 = 16.0;
/// Bound on recursive re-splitting (defensive against pathological graphs).
const SPLIT_MAX_DEPTH: usize = 3;

/// One persisted community membership for a code chunk.
pub struct CommunityResult {
    pub chunk_id: String,
    pub community_id: u32,
    pub cohesion: f32,
}

/// Detect communities over the topology and return per-code-chunk assignments.
pub fn detect(topo: &Topology) -> Vec<CommunityResult> {
    let g = &topo.graph;
    let n = g.node_count();
    if n == 0 {
        return Vec::new();
    }

    // 1. Hub exclusion (code-node utility hubs only).
    let threshold = hub_threshold(g);
    let excluded: Vec<usize> = (0..n)
        .filter(|i| !topo.container_nodes.contains(i) && g.degree(*i) > threshold)
        .collect();
    let excluded_set: HashSet<usize> = excluded.iter().copied().collect();
    let retained: Vec<usize> = (0..n).filter(|i| !excluded_set.contains(i)).collect();

    // 2/3. Partition the retained subgraph (with recursive over-size / low-cohesion splits).
    let mut next_label = 0usize;
    let retained_labels = partition(g, &retained, n, 0, &mut next_label);
    let mut label_of: HashMap<usize, usize> = retained
        .iter()
        .copied()
        .zip(retained_labels.iter().copied())
        .collect();

    // Reattach excluded hubs by majority vote over their retained neighbors.
    for &hub in &excluded {
        let mut votes: HashMap<usize, usize> = HashMap::new();
        for &(j, _) in g.neighbors(hub) {
            if let Some(&lbl) = label_of.get(&j) {
                *votes.entry(lbl).or_insert(0) += 1;
            }
        }
        let label = votes
            .into_iter()
            // most votes wins; tie → smallest label id (deterministic).
            .max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(&a.0)))
            .map(|(lbl, _)| lbl)
            .unwrap_or_else(|| {
                let l = next_label;
                next_label += 1;
                l
            });
        label_of.insert(hub, label);
    }

    // 4. Group members by final label; compute cohesion over ALL members.
    let mut members_by_label: HashMap<usize, Vec<usize>> = HashMap::new();
    for (&node, &lbl) in &label_of {
        members_by_label.entry(lbl).or_default().push(node);
    }

    // Keep only communities that contain at least one code (non-container) node.
    struct Community {
        code_nodes: Vec<usize>,
        cohesion: f32,
        min_code_id: String,
    }
    let mut communities: Vec<Community> = Vec::new();
    for (_lbl, members) in members_by_label {
        let code_nodes: Vec<usize> = members
            .iter()
            .copied()
            .filter(|i| !topo.container_nodes.contains(i))
            .collect();
        if code_nodes.is_empty() {
            continue;
        }
        let set: HashSet<usize> = members.iter().copied().collect();
        let cohesion = cohesion_score(g, &set);
        let min_code_id = code_nodes
            .iter()
            .map(|&i| topo.ids[i].as_str())
            .min()
            .unwrap_or("")
            .to_string();
        communities.push(Community {
            code_nodes,
            cohesion,
            min_code_id,
        });
    }

    // 5. Stable re-index: size (code members) desc, then min code chunk_id asc.
    communities.sort_by(|a, b| {
        b.code_nodes
            .len()
            .cmp(&a.code_nodes.len())
            .then_with(|| a.min_code_id.cmp(&b.min_code_id))
    });

    let mut out = Vec::new();
    for (cid, comm) in communities.into_iter().enumerate() {
        for node in comm.code_nodes {
            out.push(CommunityResult {
                chunk_id: topo.ids[node].clone(),
                community_id: cid as u32,
                cohesion: comm.cohesion,
            });
        }
    }
    // Deterministic output order.
    out.sort_by(|a, b| {
        a.community_id
            .cmp(&b.community_id)
            .then_with(|| a.chunk_id.cmp(&b.chunk_id))
    });
    out
}

/// Partition the induced subgraph over `nodes` (original indices), recursively
/// re-splitting oversized / low-cohesion communities. Returns a label per entry
/// in `nodes`; labels are globally unique (drawn from `*next_label`).
fn partition(
    g: &WeightedGraph,
    nodes: &[usize],
    total_nodes: usize,
    depth: usize,
    next_label: &mut usize,
) -> Vec<usize> {
    if nodes.is_empty() {
        return Vec::new();
    }
    let (sub, local_to_orig) = induced_subgraph(g, nodes);
    let local_labels = louvain(&sub);

    // Group local node indices by louvain label.
    let mut groups: HashMap<usize, Vec<usize>> = HashMap::new();
    for (local, &lbl) in local_labels.iter().enumerate() {
        groups.entry(lbl).or_default().push(local);
    }

    // Deterministic group order.
    let mut group_keys: Vec<usize> = groups.keys().copied().collect();
    group_keys.sort_unstable();
    let subdivided = group_keys.len() > 1;

    let mut result = vec![0usize; nodes.len()];
    for key in group_keys {
        let local_members = &groups[&key];
        let orig_members: Vec<usize> = local_members.iter().map(|&l| local_to_orig[l]).collect();
        let member_set: HashSet<usize> = orig_members.iter().copied().collect();

        let oversized = orig_members.len() as f64 > OVERSIZE_FRACTION * total_nodes as f64;
        let low_cohesion = orig_members.len() >= LOW_COHESION_MIN_SIZE
            && (cohesion_score(g, &member_set) as f64) < LOW_COHESION_THRESHOLD;

        // Only recurse if Louvain actually subdivided this level (otherwise we'd
        // loop forever on an irreducible community).
        if depth < SPLIT_MAX_DEPTH && subdivided && (oversized || low_cohesion) {
            let sub_labels = partition(g, &orig_members, total_nodes, depth + 1, next_label);
            for (idx, &local) in local_members.iter().enumerate() {
                result[local] = sub_labels[idx];
            }
        } else {
            let label = *next_label;
            *next_label += 1;
            for &local in local_members {
                result[local] = label;
            }
        }
    }
    result
}

/// Build the induced subgraph over `nodes`. Returns the subgraph (local indices
/// `0..nodes.len()`) and the local→original index map.
fn induced_subgraph(g: &WeightedGraph, nodes: &[usize]) -> (WeightedGraph, Vec<usize>) {
    let local_of: HashMap<usize, usize> = nodes.iter().enumerate().map(|(l, &o)| (o, l)).collect();
    let mut edges: Vec<(usize, usize, f64)> = Vec::new();
    for (local, &orig) in nodes.iter().enumerate() {
        for &(j, w) in g.neighbors(orig) {
            if let Some(&lj) = local_of.get(&j) {
                // each undirected pair once
                if lj >= local {
                    edges.push((local, lj, w));
                }
            }
        }
    }
    (
        WeightedGraph::from_edges(nodes.len(), edges),
        nodes.to_vec(),
    )
}

/// Cohesion = intra-community edges / max possible undirected pairs, in `[0, 1]`.
fn cohesion_score(g: &WeightedGraph, members: &HashSet<usize>) -> f32 {
    let k = members.len();
    if k < 2 {
        return 1.0; // a singleton/empty community is trivially cohesive
    }
    let intra = g.intra_edge_count(members) as f64;
    let max_possible = (k * (k - 1)) as f64 / 2.0;
    (intra / max_possible) as f32
}

/// Hub threshold = `max(p99 weighted degree, floor)`.
fn hub_threshold(g: &WeightedGraph) -> f64 {
    let n = g.node_count();
    if n == 0 {
        return f64::INFINITY;
    }
    let mut degrees: Vec<f64> = (0..n).map(|i| g.degree(i)).collect();
    degrees.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let idx = (((n - 1) as f64) * 0.99).floor() as usize;
    let p99 = degrees[idx];
    p99.max(HUB_DEGREE_FLOOR)
}

#[cfg(test)]
mod tests {
    use super::*;
    use code_rag_types::{CallEdge, EdgeConfidence, EdgeContext, EdgeRelation, GraphEdge};

    fn call(a: &str, b: &str) -> CallEdge {
        CallEdge {
            edge_id: format!("{a}->{b}"),
            caller_chunk_id: a.into(),
            callee_chunk_id: b.into(),
            caller_identifier: a.into(),
            callee_identifier: b.into(),
            caller_file: "f.rs".into(),
            callee_file: "f.rs".into(),
            project_name: "p".into(),
            resolution_tier: 1,
        }
    }

    fn contains_file(file: &str, func: &str, path: &str) -> GraphEdge {
        GraphEdge {
            edge_id: format!("{file}->{func}"),
            source_chunk_id: file.into(),
            target_chunk_id: func.into(),
            source_identifier: file.into(),
            target_identifier: func.into(),
            source_file: path.into(),
            target_file: path.into(),
            project_name: "p".into(),
            relation: EdgeRelation::Contains,
            context: EdgeContext::None,
            confidence: EdgeConfidence::Extracted,
        }
    }

    #[test]
    fn two_clusters_resolve_and_containers_dropped() {
        // Two call-triangles bridged by one edge, plus a file node containing all.
        let calls = vec![
            call("a", "b"),
            call("b", "c"),
            call("a", "c"),
            call("d", "e"),
            call("e", "f"),
            call("d", "f"),
            call("c", "d"), // bridge
        ];
        let edges = vec![
            contains_file("FILE", "a", "x.rs"),
            contains_file("FILE", "b", "x.rs"),
            contains_file("FILE", "c", "x.rs"),
            contains_file("FILE", "d", "x.rs"),
            contains_file("FILE", "e", "x.rs"),
            contains_file("FILE", "f", "x.rs"),
        ];
        let topo = Topology::build(&calls, &edges);
        let results = detect(&topo);

        // FILE (container) is not persisted; all 6 code chunks are.
        let ids: HashSet<&str> = results.iter().map(|r| r.chunk_id.as_str()).collect();
        assert!(!ids.contains("FILE"));
        assert_eq!(ids.len(), 6);

        // Cohesion is in range and ids are deterministic across runs.
        for r in &results {
            assert!(r.cohesion >= 0.0 && r.cohesion <= 1.0);
        }
        let again = detect(&Topology::build(&calls, &edges));
        let pairs1: Vec<(String, u32)> = results
            .iter()
            .map(|r| (r.chunk_id.clone(), r.community_id))
            .collect();
        let pairs2: Vec<(String, u32)> = again
            .iter()
            .map(|r| (r.chunk_id.clone(), r.community_id))
            .collect();
        assert_eq!(pairs1, pairs2);
    }

    #[test]
    fn empty_topology_yields_nothing() {
        let topo = Topology::build(&[], &[]);
        assert!(detect(&topo).is_empty());
    }
}
