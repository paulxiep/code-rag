//! R4: structural analytics — the numbers behind the architecture report.
//!
//! Pure (no I/O): assembles `ProjectAnalytics` from the already-built topology,
//! the R2 community assignments and the raw edges. `lib.rs` fetches, calls,
//! and hands the result to `report` for rendering. Analytics are derived data:
//! recomputed on every topology run, never persisted (there is no retrieval
//! consumer; the persisted edge tables are the source of truth).
//!
//! - **Central nodes** — weighted degree via the wasm-shared
//!   `code_rag_engine::centrality`, container (file) nodes filtered out:
//!   "functions to read first" means code chunks, and file nodes are hubs by
//!   construction.
//! - **Bridges** — high edge-betweenness edges whose endpoints sit in
//!   different communities (hidden cross-module coupling).
//! - **Surprise** — `betweenness × weight / pair_edge_count(Ca, Cb)`: an edge
//!   that is one of only a couple of links between two communities outranks
//!   one inside a thick, expected seam. A tunable heuristic, kept next to the
//!   caps below.

use std::collections::HashMap;

use code_rag_engine::centrality::degree_centrality;
use code_rag_types::{ClusterChunk, CodeChunk, GraphEdge};

use crate::betweenness::edge_betweenness;
use crate::cluster::CommunityResult;
use crate::cycles::{Cycle, find_import_cycles};
use crate::topology::Topology;

/// How many central nodes the report leads with.
const TOP_CENTRAL: usize = 10;
/// How many bridges / surprising connections the report shows.
const TOP_BRIDGES: usize = 10;

/// A bridge endpoint, labeled for human reading.
#[derive(Clone, Debug)]
pub struct EndpointLabel {
    pub chunk_id: String,
    pub identifier: String,
    pub file: String,
}

/// One entry of the "read these first" list.
#[derive(Clone, Debug)]
pub struct CentralEntry {
    /// Stable node key — not rendered in the report, but the R5 viz/export
    /// (click-a-node → query) keys on it.
    #[allow(dead_code)]
    pub chunk_id: String,
    pub identifier: String,
    pub file: String,
    pub degree: f64,
    pub community_id: Option<u32>,
}

/// One cross-community edge, scored by betweenness and surprise.
#[derive(Clone, Debug)]
pub struct Bridge {
    pub source: EndpointLabel,
    pub target: EndpointLabel,
    pub betweenness: f64,
    pub weight: f64,
    pub communities: (u32, u32),
    /// Distinct topology edges between this community pair.
    pub pair_edge_count: usize,
    pub surprise: f64,
}

/// One community row for the report (size, cohesion, concern).
#[derive(Clone, Debug)]
pub struct CommunityLine {
    pub id: u32,
    pub size: usize,
    pub cohesion: f32,
    /// Identifier of the most-central member ("likely concern").
    pub central_member: String,
    pub dominant_dir: String,
}

/// Everything the architecture report renders, minus the community table
/// (assembled separately from the R3 cluster chunks by [`community_lines`]).
#[derive(Clone, Debug)]
pub struct ProjectAnalytics {
    pub node_count: usize,
    pub edge_count: usize,
    pub central_nodes: Vec<CentralEntry>,
    pub bridges: Vec<Bridge>,
    pub surprising: Vec<Bridge>,
    pub cycles: Vec<Cycle>,
}

/// Compute the R4 analytics for one project's topology.
///
/// `project` scopes the central-node list: R1 reference resolution can resolve
/// ubiquitous identifiers (`String`, `Result`, …) to *another* project's
/// definition, and those cross-project endpoints must not lead a
/// project-scoped "read these first" list. Bridges are deliberately NOT
/// filtered — a cross-project endpoint there is visible evidence of the
/// resolution leak (follow-up tracked in the dev log).
pub fn compute(
    project: &str,
    topo: &Topology,
    results: &[CommunityResult],
    graph_edges: &[GraphEdge],
    members: &HashMap<String, CodeChunk>,
) -> ProjectAnalytics {
    let g = &topo.graph;
    let n = g.node_count();

    // node index → community, lifted to container nodes by majority vote.
    let community_of = lift_communities(topo, results);

    // Labels: prefer the fetched member chunks; fall back to identifiers
    // carried on the edge records (covers file/container nodes).
    let fallback = edge_labels(graph_edges);
    let label = |idx: usize| -> EndpointLabel {
        let chunk_id = topo.ids[idx].clone();
        if let Some(c) = members.get(&chunk_id) {
            EndpointLabel {
                chunk_id,
                identifier: c.identifier.clone(),
                file: c.file_path.clone(),
            }
        } else {
            let (identifier, file) = fallback
                .get(chunk_id.as_str())
                .map(|&(i, f)| (i.to_string(), f.to_string()))
                .unwrap_or_default();
            EndpointLabel {
                chunk_id,
                identifier,
                file,
            }
        }
    };

    // Central nodes: shared wasm-safe degree ranking, containers filtered.
    let container_ids: std::collections::HashSet<&str> = topo
        .container_nodes
        .iter()
        .map(|&i| topo.ids[i].as_str())
        .collect();
    let index_of: HashMap<&str, usize> = topo
        .ids
        .iter()
        .enumerate()
        .map(|(i, id)| (id.as_str(), i))
        .collect();
    let central_nodes: Vec<CentralEntry> = degree_centrality(
        g.edges()
            .map(|(u, v, w)| (topo.ids[u].clone(), topo.ids[v].clone(), w)),
    )
    .into_iter()
    .filter(|c| !container_ids.contains(c.id.as_str()))
    .filter(|c| {
        members
            .get(&c.id)
            .is_some_and(|m| m.project_name == project)
    })
    .take(TOP_CENTRAL)
    .map(|c| {
        let idx = index_of[c.id.as_str()];
        let l = label(idx);
        CentralEntry {
            chunk_id: l.chunk_id,
            identifier: l.identifier,
            file: l.file,
            degree: c.degree,
            community_id: community_of[idx],
        }
    })
    .collect();

    // Cross-community edges + per-pair edge counts (for the surprise score).
    let mut pair_counts: HashMap<(u32, u32), usize> = HashMap::new();
    let mut cross: Vec<(usize, usize, f64, (u32, u32))> = Vec::new();
    for (u, v, w) in g.edges() {
        if u == v {
            continue;
        }
        if let (Some(cu), Some(cv)) = (community_of[u], community_of[v])
            && cu != cv
        {
            let pair = (cu.min(cv), cu.max(cv));
            *pair_counts.entry(pair).or_insert(0) += 1;
            cross.push((u, v, w, pair));
        }
    }

    let bc = edge_betweenness(g);
    let mut bridges: Vec<Bridge> = cross
        .into_iter()
        .map(|(u, v, w, pair)| {
            let betweenness = bc.get(&(u.min(v), u.max(v))).copied().unwrap_or(0.0);
            let pair_edge_count = pair_counts[&pair];
            Bridge {
                source: label(u),
                target: label(v),
                betweenness,
                weight: w,
                communities: pair,
                pair_edge_count,
                surprise: betweenness * w / pair_edge_count as f64,
            }
        })
        .collect();

    let by_endpoints = |a: &Bridge, b: &Bridge| {
        (a.source.chunk_id.as_str(), a.target.chunk_id.as_str())
            .cmp(&(b.source.chunk_id.as_str(), b.target.chunk_id.as_str()))
    };
    bridges.sort_by(|a, b| {
        b.betweenness
            .partial_cmp(&a.betweenness)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| by_endpoints(a, b))
    });
    let mut surprising = bridges.clone();
    surprising.sort_by(|a, b| {
        b.surprise
            .partial_cmp(&a.surprise)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| by_endpoints(a, b))
    });
    bridges.truncate(TOP_BRIDGES);
    surprising.truncate(TOP_BRIDGES);

    ProjectAnalytics {
        node_count: n,
        edge_count: g.edges().filter(|&(u, v, _)| u != v).count(),
        central_nodes,
        bridges,
        surprising,
        cycles: find_import_cycles(graph_edges),
    }
}

/// One report row per community, from the R3 cluster chunks plus a fresh
/// central-member pick (highest weighted degree; tie → smallest chunk id —
/// member ids are sorted and we only replace on strictly greater degree, the
/// same rule `clusterchunk` uses). Unlike the persisted R3 summary, the pick
/// is scoped to `project`-local members — same rationale as [`compute`].
pub fn community_lines(
    project: &str,
    topo: &Topology,
    cluster_chunks: &[ClusterChunk],
    members: &HashMap<String, CodeChunk>,
) -> Vec<CommunityLine> {
    let degree_of: HashMap<&str, f64> = (0..topo.ids.len())
        .map(|i| (topo.ids[i].as_str(), topo.graph.degree(i)))
        .collect();
    let mut out: Vec<CommunityLine> = cluster_chunks
        .iter()
        .map(|cc| {
            let mut central_member = String::new();
            let mut best = f64::NEG_INFINITY;
            for id in &cc.member_chunk_ids {
                let d = degree_of.get(id.as_str()).copied().unwrap_or(0.0);
                if d > best
                    && let Some(c) = members.get(id)
                    && c.project_name == project
                {
                    best = d;
                    central_member = c.identifier.clone();
                }
            }
            CommunityLine {
                id: cc.cluster_id,
                size: cc.member_chunk_ids.len(),
                cohesion: cc.cohesion,
                central_member,
                dominant_dir: cc.path.clone(),
            }
        })
        .collect();
    out.sort_by_key(|l| l.id);
    out
}

/// Community per node index, extended from code chunks to container (file)
/// nodes by majority vote over assigned neighbors (tie → smallest community
/// id — the same rule hub reattachment uses). Import edges are usually *the*
/// cross-module coupling, and they hang off file nodes; without the lift they
/// would be invisible to bridge reporting.
fn lift_communities(topo: &Topology, results: &[CommunityResult]) -> Vec<Option<u32>> {
    let assigned: HashMap<&str, u32> = results
        .iter()
        .map(|r| (r.chunk_id.as_str(), r.community_id))
        .collect();
    let n = topo.graph.node_count();
    let mut community_of: Vec<Option<u32>> = (0..n)
        .map(|i| assigned.get(topo.ids[i].as_str()).copied())
        .collect();
    for &i in &topo.container_nodes {
        if community_of[i].is_some() {
            continue;
        }
        let mut votes: HashMap<u32, usize> = HashMap::new();
        for &(j, _) in topo.graph.neighbors(i) {
            if let Some(c) = community_of[j] {
                *votes.entry(c).or_insert(0) += 1;
            }
        }
        community_of[i] = votes
            .into_iter()
            .max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(&a.0)))
            .map(|(c, _)| c);
    }
    community_of
}

/// chunk_id → (identifier, file) harvested from graph-edge records.
fn edge_labels(graph_edges: &[GraphEdge]) -> HashMap<&str, (&str, &str)> {
    let mut map: HashMap<&str, (&str, &str)> = HashMap::new();
    for e in graph_edges {
        map.entry(e.source_chunk_id.as_str())
            .or_insert((e.source_identifier.as_str(), e.source_file.as_str()));
        map.entry(e.target_chunk_id.as_str())
            .or_insert((e.target_identifier.as_str(), e.target_file.as_str()));
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cluster;
    use code_rag_types::{CallEdge, EdgeConfidence, EdgeContext, EdgeRelation};

    fn call(a: &str, b: &str) -> CallEdge {
        CallEdge {
            edge_id: format!("{a}->{b}"),
            caller_chunk_id: a.into(),
            callee_chunk_id: b.into(),
            caller_identifier: a.into(),
            callee_identifier: b.into(),
            caller_file: "x.rs".into(),
            callee_file: "x.rs".into(),
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

    fn code(id: &str, file: &str) -> CodeChunk {
        CodeChunk {
            file_path: file.into(),
            language: "rust".into(),
            identifier: format!("fn_{id}"),
            node_type: "function_item".into(),
            code_content: "..".into(),
            start_line: 1,
            project_name: "p".into(),
            docstring: None,
            signature: None,
            chunk_id: id.into(),
            content_hash: "h".into(),
            embedding_model_version: "test".into(),
        }
    }

    /// Two call-triangles bridged by one edge — two communities, one bridge.
    fn fixture() -> (Vec<CallEdge>, Vec<GraphEdge>, HashMap<String, CodeChunk>) {
        let calls = vec![
            call("a", "b"),
            call("b", "c"),
            call("a", "c"),
            call("d", "e"),
            call("e", "f"),
            call("d", "f"),
            call("c", "d"), // bridge
        ];
        let members: HashMap<String, CodeChunk> = ["a", "b", "c", "d", "e", "f"]
            .iter()
            .map(|id| (id.to_string(), code(id, "src/x.rs")))
            .collect();
        (calls, Vec::new(), members)
    }

    #[test]
    fn single_cross_edge_is_top_bridge_and_top_surprise() {
        let (calls, graph_edges, members) = fixture();
        let topo = Topology::build(&calls, &graph_edges);
        let results = cluster::detect(&topo);
        let a = compute("p", &topo, &results, &graph_edges, &members);

        assert_eq!(a.node_count, 6);
        assert_eq!(a.edge_count, 7);
        assert!(!a.bridges.is_empty());
        let top = &a.bridges[0];
        let pair = (top.source.chunk_id.as_str(), top.target.chunk_id.as_str());
        assert!(pair == ("c", "d") || pair == ("d", "c"));
        assert_eq!(top.pair_edge_count, 1);
        assert_ne!(top.communities.0, top.communities.1);
        let top_s = &a.surprising[0];
        assert_eq!(top_s.source.chunk_id, top.source.chunk_id);
        assert!(top_s.surprise > 0.0);
        assert!(a.cycles.is_empty());
    }

    #[test]
    fn container_nodes_excluded_from_central_list() {
        let (calls, _, mut members) = fixture();
        // A file node containing every function — highest degree by construction.
        let graph_edges: Vec<GraphEdge> = ["a", "b", "c", "d", "e", "f"]
            .iter()
            .map(|f| contains_file("FILE", f, "src/x.rs"))
            .collect();
        members.insert("FILE".to_string(), code("FILE", "src/x.rs"));
        let topo = Topology::build(&calls, &graph_edges);
        let results = cluster::detect(&topo);
        let a = compute("p", &topo, &results, &graph_edges, &members);
        assert!(a.central_nodes.iter().all(|c| c.chunk_id != "FILE"));
        assert!(!a.central_nodes.is_empty());
        // Members carry labels through.
        assert!(a.central_nodes[0].identifier.starts_with("fn_"));
    }

    #[test]
    fn cross_project_chunks_excluded_from_central_list() {
        let (calls, graph_edges, mut members) = fixture();
        // "a" resolves to another project's chunk (the R1 ubiquitous-identifier
        // leak) — it must not lead this project's central list.
        members.get_mut("a").unwrap().project_name = "other".into();
        let topo = Topology::build(&calls, &graph_edges);
        let results = cluster::detect(&topo);
        let a = compute("p", &topo, &results, &graph_edges, &members);
        assert!(a.central_nodes.iter().all(|c| c.chunk_id != "a"));
        assert_eq!(a.central_nodes.len(), 5);
    }

    #[test]
    fn container_community_lifted_by_majority_vote() {
        let (calls, _, _) = fixture();
        let graph_edges = vec![
            contains_file("FILE", "a", "src/x.rs"),
            contains_file("FILE", "b", "src/x.rs"),
            contains_file("FILE", "d", "src/x.rs"),
        ];
        let topo = Topology::build(&calls, &graph_edges);
        let results = cluster::detect(&topo);
        let lifted = lift_communities(&topo, &results);
        let idx = topo.ids.iter().position(|s| s == "FILE").unwrap();
        let comm_of = |id: &str| {
            results
                .iter()
                .find(|r| r.chunk_id == id)
                .map(|r| r.community_id)
        };
        // FILE touches {a, b} (community of a) and {d} → majority vote wins.
        assert_eq!(lifted[idx], comm_of("a"));
    }

    #[test]
    fn community_lines_pick_central_member() {
        let (calls, graph_edges, members) = fixture();
        let topo = Topology::build(&calls, &graph_edges);
        let results = cluster::detect(&topo);
        let ccs = crate::clusterchunk::build_cluster_chunks(
            "p",
            &topo,
            &results,
            &members,
            &calls,
            &graph_edges,
        );
        let lines = community_lines("p", &topo, &ccs, &members);
        assert_eq!(lines.len(), ccs.len());
        for l in &lines {
            assert!(!l.central_member.is_empty());
            assert!(l.size >= 1);
        }
    }
}
