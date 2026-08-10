//! R5: GraphML export — `topology_<project>.graphml`, for offline exploration
//! in Gephi / yEd.
//!
//! Pure (no I/O), hand-rolled string writer per the crate convention for
//! markup formats: the document shape is fixed (8 declared keys), so an XML
//! dependency buys nothing, and byte-determinism stays trivially auditable.
//! The full graph is emitted — no node cap; offline tools handle scale.

use std::fmt::Write;

use crate::graph_model::ExportGraph;

/// Render one project's topology as a GraphML document.
///
/// Node ids are chunk ids (escaped); edge ids are positional `e0..eN` in the
/// model's sorted edge order. `community` is omitted on unassigned nodes.
/// Floats (`degree`, `weight`) are sums of 1.0-weight edges, so `{}`
/// formatting is stable.
pub fn render_graphml(project: &str, graph: &ExportGraph) -> String {
    let mut s = String::new();
    let w = &mut s;
    let _ = writeln!(w, r#"<?xml version="1.0" encoding="UTF-8"?>"#);
    let _ = writeln!(
        w,
        r#"<graphml xmlns="http://graphml.graphdrawing.org/xmlns">"#
    );
    let _ = writeln!(
        w,
        r#"  <key id="label" for="node" attr.name="label" attr.type="string"/>"#
    );
    let _ = writeln!(
        w,
        r#"  <key id="file" for="node" attr.name="file" attr.type="string"/>"#
    );
    let _ = writeln!(
        w,
        r#"  <key id="kind" for="node" attr.name="kind" attr.type="string"/>"#
    );
    let _ = writeln!(
        w,
        r#"  <key id="community" for="node" attr.name="community" attr.type="int"/>"#
    );
    let _ = writeln!(
        w,
        r#"  <key id="degree" for="node" attr.name="degree" attr.type="double"/>"#
    );
    let _ = writeln!(
        w,
        r#"  <key id="relation" for="edge" attr.name="relation" attr.type="string"/>"#
    );
    let _ = writeln!(
        w,
        r#"  <key id="confidence" for="edge" attr.name="confidence" attr.type="string"/>"#
    );
    let _ = writeln!(
        w,
        r#"  <key id="weight" for="edge" attr.name="weight" attr.type="double"/>"#
    );
    let _ = writeln!(
        w,
        r#"  <graph id="{}" edgedefault="undirected">"#,
        xml_escape(project)
    );
    for n in &graph.nodes {
        let _ = writeln!(w, r#"    <node id="{}">"#, xml_escape(&n.id));
        let _ = writeln!(
            w,
            r#"      <data key="label">{}</data>"#,
            xml_escape(&n.label)
        );
        let _ = writeln!(
            w,
            r#"      <data key="file">{}</data>"#,
            xml_escape(&n.file)
        );
        let _ = writeln!(w, r#"      <data key="kind">{}</data>"#, n.kind.as_str());
        if let Some(c) = n.community {
            let _ = writeln!(w, r#"      <data key="community">{c}</data>"#);
        }
        let _ = writeln!(w, r#"      <data key="degree">{}</data>"#, n.degree);
        let _ = writeln!(w, r#"    </node>"#);
    }
    for (i, e) in graph.edges.iter().enumerate() {
        let _ = writeln!(
            w,
            r#"    <edge id="e{i}" source="{}" target="{}">"#,
            xml_escape(&e.source),
            xml_escape(&e.target)
        );
        let _ = writeln!(
            w,
            r#"      <data key="relation">{}</data>"#,
            xml_escape(&e.relations.join("+"))
        );
        let _ = writeln!(w, r#"      <data key="confidence">{}</data>"#, e.confidence);
        let _ = writeln!(w, r#"      <data key="weight">{}</data>"#, e.weight);
        let _ = writeln!(w, r#"    </edge>"#);
    }
    let _ = writeln!(w, r#"  </graph>"#);
    let _ = writeln!(w, r#"</graphml>"#);
    s
}

/// Escape text for XML content and attribute values (`&` first).
fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph_model::{ExportNode, ExportPairEdge, NodeKind};

    fn fixture() -> ExportGraph {
        ExportGraph {
            nodes: vec![
                ExportNode {
                    id: "a".into(),
                    label: "Vec<T> & \"x\"".into(),
                    file: "src/a.rs".into(),
                    kind: NodeKind::Code,
                    community: Some(3),
                    degree: 2.0,
                },
                ExportNode {
                    id: "b".into(),
                    label: "fn_b".into(),
                    file: "src/b.rs".into(),
                    kind: NodeKind::File,
                    community: None,
                    degree: 1.0,
                },
            ],
            edges: vec![ExportPairEdge {
                source: "a".into(),
                target: "b".into(),
                relations: vec!["calls", "references"],
                confidence: "extracted",
                weight: 2.0,
            }],
        }
    }

    #[test]
    fn golden_fragments() {
        let xml = render_graphml("p", &fixture());
        assert!(xml.starts_with(r#"<?xml version="1.0" encoding="UTF-8"?>"#));
        assert!(
            xml.contains(
                r#"<key id="community" for="node" attr.name="community" attr.type="int"/>"#
            )
        );
        assert!(xml.contains(r#"<graph id="p" edgedefault="undirected">"#));
        assert!(xml.contains(r#"<node id="a">"#));
        assert!(xml.contains(r#"<data key="degree">2</data>"#));
        assert!(xml.contains(r#"<edge id="e0" source="a" target="b">"#));
        assert!(xml.contains(r#"<data key="relation">calls+references</data>"#));
        assert!(xml.ends_with("</graphml>\n"));
    }

    #[test]
    fn community_omitted_when_unassigned() {
        let xml = render_graphml("p", &fixture());
        let node_b = xml.split(r#"<node id="b">"#).nth(1).unwrap();
        let node_b = &node_b[..node_b.find("</node>").unwrap()];
        assert!(!node_b.contains("community"));
    }

    #[test]
    fn escapes_content() {
        let xml = render_graphml("p", &fixture());
        assert!(xml.contains(r#"<data key="label">Vec&lt;T&gt; &amp; &quot;x&quot;</data>"#));
        assert!(!xml.contains("Vec<T>"));
    }

    #[test]
    fn xml_escape_covers_all_five() {
        assert_eq!(xml_escape(r#"&<>"'"#), "&amp;&lt;&gt;&quot;&apos;");
    }

    #[test]
    fn render_is_byte_deterministic() {
        assert_eq!(
            render_graphml("p", &fixture()),
            render_graphml("p", &fixture())
        );
    }

    #[test]
    fn empty_graph_renders_valid_document() {
        let g = ExportGraph {
            nodes: vec![],
            edges: vec![],
        };
        let xml = render_graphml("empty", &g);
        assert!(xml.contains(r#"<graph id="empty" edgedefault="undirected">"#));
        assert!(xml.contains("</graphml>"));
    }
}
