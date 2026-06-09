//! Build the partitionable relation topology from persisted edges (Track R, R2).
//!
//! Pure (no I/O): takes already-fetched `call_edges` + `graph_edges` and projects
//! them onto one weighted, undirected [`WeightedGraph`]. `lib.rs` does the LanceDB
//! read; this module is unit-testable in isolation.
//!
//! **Node set & edge selection (the R2 design decision).** Communities are
//! detected over emergent *dependency* structure, not the folder tree:
//!
//! - **Kept:** `calls` (projected from `call_edges`), `imports` / `re_exports`,
//!   `implements` / `extends` / `embeds` / `references`, and **file→function
//!   `contains`** (functions in one file are usually cohesive).
//! - **Dropped:** **folder→file `contains`** — high-level folders are frequently
//!   not cohesive, so feeding the folder tree into clustering would make
//!   communities recover the folders and make R5's emergent-vs-folder comparison
//!   self-fulfilling. Folders never become graph nodes at all.
//!
//! A `contains` edge is file-level iff `source_file == target_file` (the file
//! chunk and the definition share a path); folder-level otherwise. File nodes
//! stay in the graph as connectors (carrying file-level cohesion + import
//! signal) but are flagged as `container_nodes` so they are excluded from
//! persistence — only code chunks get a community id.

use std::collections::{HashMap, HashSet};

use code_rag_types::{CallEdge, EdgeRelation, GraphEdge};

use crate::louvain::WeightedGraph;

/// The in-memory topology: a weighted undirected graph plus the chunk-id ↔ node
/// mapping and the set of container (file) nodes that must not be persisted.
pub struct Topology {
    /// node index → chunk_id (sorted, for deterministic indexing).
    pub ids: Vec<String>,
    /// The partitionable graph (all relations at equal weight; multi-edges summed).
    pub graph: WeightedGraph,
    /// Node indices that are file/container nodes — kept in the graph as
    /// connectors but dropped from community persistence.
    pub container_nodes: HashSet<usize>,
}

impl Topology {
    /// Project the edge tables onto a single undirected graph per the R2 rules.
    pub fn build(call_edges: &[CallEdge], graph_edges: &[GraphEdge]) -> Self {
        let mut pairs: Vec<(&str, &str)> = Vec::new();
        let mut container_ids: HashSet<&str> = HashSet::new();

        // calls — both endpoints are code chunks.
        for e in call_edges {
            if e.caller_chunk_id != e.callee_chunk_id {
                pairs.push((e.caller_chunk_id.as_str(), e.callee_chunk_id.as_str()));
            }
        }

        for e in graph_edges {
            if e.source_chunk_id == e.target_chunk_id {
                continue;
            }
            let keep = match e.relation {
                // Calls are projected from call_edges; a Calls graph_edge should
                // never exist, but guard against double-counting if one does.
                EdgeRelation::Calls => false,
                EdgeRelation::Contains => {
                    let file_level = e.source_file == e.target_file;
                    if file_level {
                        // source is the FileChunk node — a container/connector.
                        container_ids.insert(e.source_chunk_id.as_str());
                    }
                    file_level
                }
                EdgeRelation::Imports | EdgeRelation::ReExports => {
                    // source is the importing file's FileChunk node.
                    container_ids.insert(e.source_chunk_id.as_str());
                    true
                }
                EdgeRelation::Implements
                | EdgeRelation::Extends
                | EdgeRelation::Embeds
                | EdgeRelation::References
                | EdgeRelation::RationaleFor => true,
            };
            if keep {
                pairs.push((e.source_chunk_id.as_str(), e.target_chunk_id.as_str()));
            }
        }

        // Deterministic node indexing: sorted chunk ids.
        let mut id_set: HashSet<&str> = HashSet::new();
        for &(u, v) in &pairs {
            id_set.insert(u);
            id_set.insert(v);
        }
        let mut ids: Vec<String> = id_set.into_iter().map(|s| s.to_string()).collect();
        ids.sort();
        let index: HashMap<String, usize> = ids
            .iter()
            .enumerate()
            .map(|(i, s)| (s.clone(), i))
            .collect();

        let edges = pairs
            .iter()
            .map(|&(u, v)| (index[u], index[v], 1.0_f64));
        let graph = WeightedGraph::from_edges(ids.len(), edges);

        let container_nodes: HashSet<usize> = container_ids
            .iter()
            .filter_map(|s| index.get(*s).copied())
            .collect();

        Topology {
            ids,
            graph,
            container_nodes,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.graph.node_count() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use code_rag_types::{EdgeConfidence, EdgeContext};

    fn ge(src: &str, tgt: &str, src_file: &str, tgt_file: &str, rel: EdgeRelation) -> GraphEdge {
        GraphEdge {
            edge_id: format!("{src}->{tgt}:{}", rel.as_str()),
            source_chunk_id: src.into(),
            target_chunk_id: tgt.into(),
            source_identifier: src.into(),
            target_identifier: tgt.into(),
            source_file: src_file.into(),
            target_file: tgt_file.into(),
            project_name: "p".into(),
            relation: rel,
            context: EdgeContext::None,
            confidence: EdgeConfidence::Extracted,
        }
    }

    #[test]
    fn folder_contains_dropped_file_contains_kept() {
        let edges = vec![
            // folder→file contains: source_file (folder) != target_file (file) → dropped.
            ge("folderA", "fileX", "proj/src", "proj/src/x.rs", EdgeRelation::Contains),
            // file→function contains: same path → kept, source flagged container.
            ge("fileX", "fnA", "proj/src/x.rs", "proj/src/x.rs", EdgeRelation::Contains),
        ];
        let topo = Topology::build(&[], &edges);
        // folderA must not be a node; fileX + fnA are nodes; fileX is a container.
        let pos = |id: &str| topo.ids.iter().position(|s| s == id);
        assert!(pos("folderA").is_none());
        assert!(pos("fileX").is_some());
        assert!(pos("fnA").is_some());
        assert!(topo.container_nodes.contains(&pos("fileX").unwrap()));
        assert!(!topo.container_nodes.contains(&pos("fnA").unwrap()));
    }

    #[test]
    fn deterministic_node_indexing() {
        let edges = vec![
            ge("b", "a", "f.rs", "f.rs", EdgeRelation::References),
            ge("c", "a", "f.rs", "f.rs", EdgeRelation::References),
        ];
        let t1 = Topology::build(&[], &edges);
        let t2 = Topology::build(&[], &edges);
        assert_eq!(t1.ids, t2.ids);
        // sorted: a, b, c
        assert_eq!(t1.ids, vec!["a", "b", "c"]);
    }
}
