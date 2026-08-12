//! R5: the export-facing graph model — one assembly both the viz JSON and the
//! GraphML writer consume.
//!
//! Pure (no I/O): projects the in-memory [`Topology`] plus community results
//! and raw edge records into labeled nodes and per-pair annotated edges.
//! Node order is the topology's sorted chunk-id order; edge order is ascending
//! `(source, target)` — both writers inherit determinism from here.

use std::collections::{BTreeSet, HashMap};

use code_rag_types::{CallEdge, CodeChunk, EdgeRelation, GraphEdge};

use crate::analytics::{edge_labels, lift_communities};
use crate::cluster::CommunityResult;
use crate::topology::Topology;

/// Call-edge resolution tiers 1 (same-file) and 2 (import-anchored) are
/// AST/import-proven; tier 3 (unique-in-project) is a heuristic — the same
/// split `EdgeConfidence` records for graph edges.
const CALL_TIER_EXTRACTED_MAX: u8 = 2;

/// What a topology node is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    /// A code definition (function, type, …) — carries a community id.
    Code,
    /// A file/container connector node.
    File,
}

impl NodeKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            NodeKind::Code => "code",
            NodeKind::File => "file",
        }
    }
}

/// One export node: a topology node with its label, kind, community, degree.
#[derive(Debug, Clone)]
pub struct ExportNode {
    /// chunk_id — the stable identity the click-to-query flow keys on.
    pub id: String,
    pub label: String,
    pub file: String,
    pub kind: NodeKind,
    /// Community id; containers get their majority-vote lifted id.
    pub community: Option<u32>,
    /// Weighted degree in the topology graph.
    pub degree: f64,
}

/// One export edge per unordered node pair, annotated with every relation the
/// raw edges carry for that pair and a folded confidence.
#[derive(Debug, Clone)]
pub struct ExportPairEdge {
    /// Lexically smaller chunk_id.
    pub source: String,
    /// Lexically larger chunk_id.
    pub target: String,
    /// Sorted, deduped relation tags (`calls`, `imports`, …).
    pub relations: Vec<&'static str>,
    /// `"extracted"` if any contributing raw edge is AST/import-proven,
    /// `"inferred"` otherwise.
    pub confidence: &'static str,
    /// Summed weight in the topology graph (multi-edges accumulate).
    pub weight: f64,
}

/// The export-ready graph.
#[derive(Debug, Clone)]
pub struct ExportGraph {
    pub nodes: Vec<ExportNode>,
    pub edges: Vec<ExportPairEdge>,
}

/// Assemble the export graph for one project's topology.
pub fn build_export_graph(
    topo: &Topology,
    results: &[CommunityResult],
    members: &HashMap<String, CodeChunk>,
    call_edges: &[CallEdge],
    graph_edges: &[GraphEdge],
) -> ExportGraph {
    let community_of = lift_communities(topo, results);
    let fallback = edge_labels(graph_edges);

    // topo.ids is sorted → iterating by index yields nodes sorted by id.
    let nodes: Vec<ExportNode> = (0..topo.ids.len())
        .map(|i| {
            let id = topo.ids[i].clone();
            let (label, file) = if let Some(c) = members.get(&id) {
                (c.identifier.clone(), c.file_path.clone())
            } else {
                fallback
                    .get(id.as_str())
                    .map(|&(l, f)| (l.to_string(), f.to_string()))
                    .unwrap_or_default()
            };
            ExportNode {
                id,
                label,
                file,
                kind: if topo.container_nodes.contains(&i) {
                    NodeKind::File
                } else {
                    NodeKind::Code
                },
                community: community_of[i],
                degree: topo.graph.degree(i),
            }
        })
        .collect();

    let annotations = pair_annotations(call_edges, graph_edges);
    // `edges()` yields ascending (u, v) with u <= v; sorted ids make that
    // ascending (source, target) too. Self-loops are dropped.
    let edges: Vec<ExportPairEdge> = topo
        .graph
        .edges()
        .filter(|&(u, v, _)| u != v)
        .map(|(u, v, w)| {
            let source = topo.ids[u].clone();
            let target = topo.ids[v].clone();
            let (relations, extracted) = annotations
                .get(&(source.clone(), target.clone()))
                .map(|(tags, extracted)| (tags.iter().copied().collect(), *extracted))
                .unwrap_or_default();
            ExportPairEdge {
                source,
                target,
                relations,
                confidence: if extracted { "extracted" } else { "inferred" },
                weight: w,
            }
        })
        .collect();

    ExportGraph { nodes, edges }
}

/// Per unordered chunk-id pair: relation tags + whether any contributing raw
/// edge is extracted-confidence. Mirrors `Topology::build`'s keep-rules (the
/// confidence-aware sibling of `analytics::edge_relations`).
fn pair_annotations(
    call_edges: &[CallEdge],
    graph_edges: &[GraphEdge],
) -> HashMap<(String, String), (BTreeSet<&'static str>, bool)> {
    let mut map: HashMap<(String, String), (BTreeSet<&'static str>, bool)> = HashMap::new();
    let key = |a: &str, b: &str| (a.min(b).to_string(), a.max(b).to_string());
    for e in call_edges {
        if e.caller_chunk_id != e.callee_chunk_id {
            let entry = map
                .entry(key(&e.caller_chunk_id, &e.callee_chunk_id))
                .or_default();
            entry.0.insert("calls");
            entry.1 |= e.resolution_tier <= CALL_TIER_EXTRACTED_MAX;
        }
    }
    for e in graph_edges {
        if e.source_chunk_id == e.target_chunk_id || e.relation == EdgeRelation::Calls {
            continue;
        }
        if e.relation == EdgeRelation::Contains && e.source_file != e.target_file {
            continue; // folder-level containment — not a topology edge
        }
        let entry = map
            .entry(key(&e.source_chunk_id, &e.target_chunk_id))
            .or_default();
        entry.0.insert(e.relation.as_str());
        entry.1 |= e.confidence == code_rag_types::EdgeConfidence::Extracted;
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;
    use code_rag_types::{EdgeConfidence, EdgeContext};

    fn call(a: &str, b: &str, tier: u8) -> CallEdge {
        CallEdge {
            edge_id: format!("{a}->{b}"),
            caller_chunk_id: a.into(),
            callee_chunk_id: b.into(),
            caller_identifier: format!("fn_{a}"),
            callee_identifier: format!("fn_{b}"),
            caller_file: "src/x.rs".into(),
            callee_file: "src/x.rs".into(),
            project_name: "p".into(),
            resolution_tier: tier,
        }
    }

    fn ge(
        source: &str,
        target: &str,
        relation: EdgeRelation,
        confidence: EdgeConfidence,
    ) -> GraphEdge {
        GraphEdge {
            edge_id: format!("{source}~{target}"),
            source_chunk_id: source.into(),
            target_chunk_id: target.into(),
            source_identifier: format!("id_{source}"),
            target_identifier: format!("id_{target}"),
            source_file: "src/f.rs".into(),
            target_file: "src/g.rs".into(),
            project_name: "p".into(),
            relation,
            context: EdgeContext::None,
            confidence,
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

    fn result(id: &str, community: u32) -> CommunityResult {
        CommunityResult {
            chunk_id: id.into(),
            community_id: community,
            cohesion: 0.5,
        }
    }

    #[test]
    fn assembles_labels_kinds_communities_and_annotations() {
        // a→b calls (tier 1); file imports a (container source, Inferred).
        let calls = vec![call("a", "b", 1)];
        let graph_edges = vec![ge(
            "file",
            "a",
            EdgeRelation::Imports,
            EdgeConfidence::Inferred,
        )];
        let members: HashMap<String, CodeChunk> = [
            ("a".to_string(), code("a", "src/x.rs")),
            ("b".to_string(), code("b", "src/x.rs")),
        ]
        .into_iter()
        .collect();
        let topo = Topology::build(&calls, &graph_edges);
        let results = vec![result("a", 0), result("b", 0)];
        let g = build_export_graph(&topo, &results, &members, &calls, &graph_edges);

        assert_eq!(
            g.nodes.iter().map(|n| n.id.as_str()).collect::<Vec<_>>(),
            vec!["a", "b", "file"]
        );
        let a = &g.nodes[0];
        assert_eq!(a.label, "fn_a");
        assert_eq!(a.kind, NodeKind::Code);
        assert_eq!(a.community, Some(0));
        let file = &g.nodes[2];
        assert_eq!(file.kind, NodeKind::File);
        // Container label falls back to the edge record.
        assert_eq!(file.label, "id_file");
        // Import-only neighbor lifts the container into community 0.
        assert_eq!(file.community, Some(0));

        assert_eq!(g.edges.len(), 2);
        let ab = g
            .edges
            .iter()
            .find(|e| e.source == "a" && e.target == "b")
            .unwrap();
        assert_eq!(ab.relations, vec!["calls"]);
        assert_eq!(ab.confidence, "extracted");
        let import = g.edges.iter().find(|e| e.target == "file").unwrap();
        assert_eq!(import.source, "a");
        assert_eq!(import.relations, vec!["imports"]);
        assert_eq!(import.confidence, "inferred");
    }

    #[test]
    fn tier3_calls_and_extracted_graph_edges_fold_confidence() {
        let calls = vec![call("a", "b", 3)];
        let topo = Topology::build(&calls, &[]);
        let g = build_export_graph(&topo, &[], &HashMap::new(), &calls, &[]);
        assert_eq!(g.edges[0].confidence, "inferred");

        // Same pair also carries an Extracted References edge → extracted wins.
        let ges = vec![ge(
            "a",
            "b",
            EdgeRelation::References,
            EdgeConfidence::Extracted,
        )];
        let topo2 = Topology::build(&calls, &ges);
        let g2 = build_export_graph(&topo2, &[], &HashMap::new(), &calls, &ges);
        assert_eq!(g2.edges[0].relations, vec!["calls", "references"]);
        assert_eq!(g2.edges[0].confidence, "extracted");
    }

    #[test]
    fn deterministic_across_runs() {
        let calls = vec![call("a", "b", 1), call("b", "c", 2), call("a", "c", 3)];
        let members: HashMap<String, CodeChunk> = ["a", "b", "c"]
            .iter()
            .map(|id| (id.to_string(), code(id, "src/x.rs")))
            .collect();
        let topo = Topology::build(&calls, &[]);
        let results = vec![result("a", 0), result("b", 0), result("c", 0)];
        let one = build_export_graph(&topo, &results, &members, &calls, &[]);
        let two = build_export_graph(&topo, &results, &members, &calls, &[]);
        assert_eq!(format!("{one:?}"), format!("{two:?}"));
    }

    #[test]
    fn empty_topology_yields_empty_graph() {
        let topo = Topology::build(&[], &[]);
        let g = build_export_graph(&topo, &[], &HashMap::new(), &[], &[]);
        assert!(g.nodes.is_empty());
        assert!(g.edges.is_empty());
    }
}
