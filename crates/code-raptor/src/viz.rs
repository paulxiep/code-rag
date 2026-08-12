//! R5: the browser viz artifact — `graph_viz_<project>.json`.
//!
//! Pure (no I/O): caps the [`ExportGraph`] for browser rendering and
//! serializes it. Schema v1 is the contract with `code-rag-ui`'s topology
//! view (`viz_data.rs` mirrors it): nodes are community-colored + degree-sized
//! there, edges styled by relation/confidence; color itself is presentation
//! and deliberately lives UI-side. Byte-deterministic: input order is the
//! sorted `graph_model` order, serde emits struct fields in declaration order,
//! and no map types appear in the serialized shape.

use serde::Serialize;

use crate::analytics::CommunityLine;
use crate::graph_model::{ExportGraph, ExportPairEdge};

/// Browser node cap: beyond this, force layout and canvas hit-testing degrade.
pub const NODE_CAP: usize = 5000;
/// Browser edge cap, applied after the node cap.
pub const EDGE_CAP: usize = 15_000;

/// Bumped on any breaking change; the UI checks it before rendering.
const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Serialize)]
pub struct VizFile {
    schema_version: u32,
    project: String,
    /// Pre-cap totals, so the UI can report what truncation dropped.
    node_total: usize,
    edge_total: usize,
    truncated: bool,
    communities: Vec<VizCommunity>,
    nodes: Vec<VizNode>,
    edges: Vec<VizEdge>,
}

/// Legend row: everything the UI needs without walking the node list.
#[derive(Debug, Serialize)]
struct VizCommunity {
    id: u32,
    size: usize,
    cohesion: f32,
    /// The community's most-central member — its "likely concern".
    label: String,
    /// The community's dominant directory.
    dir: String,
}

#[derive(Debug, Serialize)]
struct VizNode {
    id: String,
    label: String,
    file: String,
    kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    community: Option<u32>,
    degree: f64,
}

#[derive(Debug, Serialize)]
struct VizEdge {
    source: String,
    target: String,
    relations: Vec<&'static str>,
    confidence: &'static str,
    weight: f64,
}

/// Build the (possibly capped) viz artifact for one project.
pub fn build_viz(
    project: &str,
    graph: &ExportGraph,
    communities: &[CommunityLine],
    node_cap: usize,
    edge_cap: usize,
) -> VizFile {
    let node_total = graph.nodes.len();
    let edge_total = graph.edges.len();

    // Node cap: keep the top-degree nodes (tie → smaller id), then restore id
    // order so the artifact stays sorted.
    let mut kept: Vec<&crate::graph_model::ExportNode> = graph.nodes.iter().collect();
    if kept.len() > node_cap {
        kept.sort_by(|a, b| b.degree.total_cmp(&a.degree).then_with(|| a.id.cmp(&b.id)));
        kept.truncate(node_cap);
        kept.sort_by(|a, b| a.id.cmp(&b.id));
    }
    let kept_ids: std::collections::HashSet<&str> = kept.iter().map(|n| n.id.as_str()).collect();

    // Drop edges with a dropped endpoint, then apply the edge cap: keep the
    // highest-value classes (extracted > inferred > inferred references-only);
    // within a class, lexically later pairs are dropped first.
    let mut edges: Vec<&ExportPairEdge> = graph
        .edges
        .iter()
        .filter(|e| kept_ids.contains(e.source.as_str()) && kept_ids.contains(e.target.as_str()))
        .collect();
    if edges.len() > edge_cap {
        edges.sort_by(|a, b| {
            keep_class(b).cmp(&keep_class(a)).then_with(|| {
                (a.source.as_str(), a.target.as_str()).cmp(&(b.source.as_str(), b.target.as_str()))
            })
        });
        edges.truncate(edge_cap);
        edges.sort_by(|a, b| {
            (a.source.as_str(), a.target.as_str()).cmp(&(b.source.as_str(), b.target.as_str()))
        });
    }

    let truncated = kept.len() < node_total || edges.len() < edge_total;
    VizFile {
        schema_version: SCHEMA_VERSION,
        project: project.to_string(),
        node_total,
        edge_total,
        truncated,
        communities: communities
            .iter()
            .map(|c| VizCommunity {
                id: c.id,
                size: c.size,
                cohesion: c.cohesion,
                label: c.central_member.clone(),
                dir: c.dominant_dir.clone(),
            })
            .collect(),
        nodes: kept
            .into_iter()
            .map(|n| VizNode {
                id: n.id.clone(),
                label: n.label.clone(),
                file: n.file.clone(),
                kind: n.kind.as_str(),
                community: n.community,
                degree: n.degree,
            })
            .collect(),
        edges: edges
            .into_iter()
            .map(|e| VizEdge {
                source: e.source.clone(),
                target: e.target.clone(),
                relations: e.relations.clone(),
                confidence: e.confidence,
                weight: e.weight,
            })
            .collect(),
    }
}

/// Edge keep-priority under the cap: 2 extracted, 1 inferred, 0 inferred
/// references-only (the noisiest tier — heuristic type references).
fn keep_class(e: &ExportPairEdge) -> u8 {
    if e.confidence == "extracted" {
        2
    } else if e.relations == ["references"] {
        0
    } else {
        1
    }
}

/// Serialize the artifact (single line; the file is machine-consumed).
pub fn render_json(viz: &VizFile) -> String {
    serde_json::to_string(viz).expect("viz file shape serializes infallibly")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph_model::{ExportNode, NodeKind};

    fn node(id: &str, degree: f64) -> ExportNode {
        ExportNode {
            id: id.into(),
            label: format!("fn_{id}"),
            file: "src/x.rs".into(),
            kind: NodeKind::Code,
            community: Some(0),
            degree,
        }
    }

    fn edge(source: &str, target: &str, confidence: &'static str) -> ExportPairEdge {
        ExportPairEdge {
            source: source.into(),
            target: target.into(),
            relations: vec!["calls"],
            confidence,
            weight: 1.0,
        }
    }

    fn fixture() -> ExportGraph {
        ExportGraph {
            nodes: vec![
                node("a", 5.0),
                node("b", 4.0),
                node("c", 3.0),
                node("d", 2.0),
                node("e", 1.0),
            ],
            edges: vec![
                edge("a", "b", "extracted"),
                edge("a", "c", "extracted"),
                edge("b", "c", "inferred"),
                edge("d", "e", "extracted"),
            ],
        }
    }

    #[test]
    fn node_cap_keeps_top_degree_and_drops_dangling_edges() {
        let viz = build_viz("p", &fixture(), &[], 3, EDGE_CAP);
        assert!(viz.truncated);
        assert_eq!(viz.node_total, 5);
        assert_eq!(viz.edge_total, 4);
        assert_eq!(
            viz.nodes.iter().map(|n| n.id.as_str()).collect::<Vec<_>>(),
            vec!["a", "b", "c"]
        );
        // d–e dangles → dropped; the three intra-top edges survive.
        assert_eq!(viz.edges.len(), 3);
        assert!(viz.edges.iter().all(|e| e.source != "d" && e.target != "e"));
    }

    #[test]
    fn edge_cap_drops_low_value_classes_first() {
        let mut g = fixture();
        g.edges.push(ExportPairEdge {
            source: "a".into(),
            target: "d".into(),
            relations: vec!["references"],
            confidence: "inferred",
            weight: 1.0,
        });
        let viz = build_viz("p", &g, &[], NODE_CAP, 3);
        assert!(viz.truncated);
        assert_eq!(viz.edges.len(), 3);
        // Both inferred edges (references-only first) were dropped.
        assert!(viz.edges.iter().all(|e| e.confidence == "extracted"));
        // Output restored to (source, target) order.
        let pairs: Vec<_> = viz
            .edges
            .iter()
            .map(|e| (e.source.as_str(), e.target.as_str()))
            .collect();
        let mut sorted = pairs.clone();
        sorted.sort();
        assert_eq!(pairs, sorted);
    }

    #[test]
    fn uncapped_graph_is_not_truncated() {
        let viz = build_viz("p", &fixture(), &[], NODE_CAP, EDGE_CAP);
        assert!(!viz.truncated);
        assert_eq!(viz.nodes.len(), 5);
        assert_eq!(viz.edges.len(), 4);
    }

    #[test]
    fn render_is_byte_deterministic() {
        let lines = vec![CommunityLine {
            id: 0,
            size: 5,
            cohesion: 0.4,
            central_member: "fn_a".into(),
            dominant_dir: "src".into(),
        }];
        let one = render_json(&build_viz("p", &fixture(), &lines, 3, 3));
        let two = render_json(&build_viz("p", &fixture(), &lines, 3, 3));
        assert_eq!(one, two);
    }

    #[test]
    fn empty_graph_renders_valid_empty_artifact() {
        let g = ExportGraph {
            nodes: vec![],
            edges: vec![],
        };
        let json = render_json(&build_viz("p", &g, &[], NODE_CAP, EDGE_CAP));
        assert!(json.contains("\"schema_version\":1"));
        assert!(json.contains("\"nodes\":[]"));
        assert!(json.contains("\"truncated\":false"));
    }

    #[test]
    fn community_none_is_omitted() {
        let mut g = fixture();
        g.nodes[0].community = None;
        let json = render_json(&build_viz("p", &g, &[], NODE_CAP, EDGE_CAP));
        // First node serialized without a community key.
        let first_node = json.split("\"nodes\":[").nth(1).unwrap();
        let first = &first_node[..first_node.find('}').unwrap()];
        assert!(!first.contains("community"));
    }
}
