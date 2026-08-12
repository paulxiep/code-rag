//! R5: the MCP-facing insights facade — the one deliberate public surface
//! over this crate's pure analytics.
//!
//! `code-rag-mcp`'s topology tools consume persisted rows (edge tables,
//! `community_assignments`, `cluster_chunks`) through the store seam and hand
//! the slices here; every function is pure and cheap — no Louvain re-run, no
//! betweenness — so tool responses match the persisted partition and stay
//! sub-second. The heavier report path (`build_topology`) does not go through
//! this module.

use std::collections::HashMap;

use code_rag_types::{CallEdge, CodeChunk, CommunityAssignment, GraphEdge};

pub use crate::analytics::CentralEntry;
pub use crate::cycles::{Cycle, find_import_cycles};
pub use crate::drift::{CommunityDrift, Divergence, DriftReport, FolderDrift};

use crate::cluster::CommunityResult;
use crate::topology::Topology;

/// Persisted assignments → the in-crate community-result shape.
fn to_results(assignments: &[CommunityAssignment]) -> Vec<CommunityResult> {
    assignments
        .iter()
        .map(|a| CommunityResult {
            chunk_id: a.chunk_id.clone(),
            community_id: a.community_id,
            cohesion: a.cohesion,
        })
        .collect()
}

/// The "read these first" ranking over one project's persisted edges:
/// weighted-degree centrality, containers and cross-project chunks excluded,
/// community ids attached from the persisted `assignments` (majority-vote
/// lifted for containers, same as the report).
pub fn central_nodes(
    project: &str,
    call_edges: &[CallEdge],
    graph_edges: &[GraphEdge],
    assignments: &[CommunityAssignment],
    members: &HashMap<String, CodeChunk>,
    limit: usize,
) -> Vec<CentralEntry> {
    let topo = Topology::build(call_edges, graph_edges);
    let community_of = crate::analytics::lift_communities(&topo, &to_results(assignments));
    crate::analytics::central_entries(project, &topo, &community_of, members, graph_edges, limit)
}

/// Emergent-vs-folder drift for one project, from persisted assignments +
/// member chunks (`get_chunks_by_ids` over the assignment ids).
pub fn drift(
    project: &str,
    assignments: &[CommunityAssignment],
    members: &HashMap<String, CodeChunk>,
) -> DriftReport {
    crate::drift::compare(project, &to_results(assignments), members)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(a: &str, b: &str) -> CallEdge {
        CallEdge {
            edge_id: format!("{a}->{b}"),
            caller_chunk_id: a.into(),
            callee_chunk_id: b.into(),
            caller_identifier: format!("fn_{a}"),
            callee_identifier: format!("fn_{b}"),
            caller_file: "src/x.rs".into(),
            callee_file: "src/x.rs".into(),
            project_name: "p".into(),
            resolution_tier: 1,
        }
    }

    fn code(id: &str, project: &str) -> CodeChunk {
        CodeChunk {
            file_path: "src/x.rs".into(),
            language: "rust".into(),
            identifier: format!("fn_{id}"),
            node_type: "function_item".into(),
            code_content: "..".into(),
            start_line: 1,
            project_name: project.into(),
            docstring: None,
            signature: None,
            chunk_id: id.into(),
            content_hash: "h".into(),
            embedding_model_version: "test".into(),
        }
    }

    fn assignment(id: &str, community: u32) -> CommunityAssignment {
        CommunityAssignment {
            project_name: "p".into(),
            chunk_id: id.into(),
            community_id: community,
            cohesion: 0.5,
        }
    }

    /// Two triangles bridged at c→d: `a` (degree 2 + bridge participation)
    /// ranks; foreign-project `d` is excluded even though its degree matches.
    #[test]
    fn central_nodes_filters_and_limits() {
        let calls = vec![
            call("a", "b"),
            call("b", "c"),
            call("a", "c"),
            call("d", "e"),
            call("e", "f"),
            call("d", "f"),
            call("c", "d"),
        ];
        let mut members: HashMap<String, CodeChunk> = ["a", "b", "c"]
            .iter()
            .map(|id| (id.to_string(), code(id, "p")))
            .collect();
        for id in ["d", "e", "f"] {
            members.insert(id.to_string(), code(id, "other"));
        }
        let assignments: Vec<_> = [("a", 0), ("b", 0), ("c", 0)]
            .iter()
            .map(|&(id, c)| assignment(id, c))
            .collect();

        let all = central_nodes("p", &calls, &[], &assignments, &members, 10);
        // Only project-local chunks survive the filter.
        assert_eq!(all.len(), 3);
        assert!(
            all.iter()
                .all(|e| ["a", "b", "c"].contains(&e.chunk_id.as_str()))
        );
        // Community ids come from the persisted assignments.
        assert!(all.iter().all(|e| e.community_id == Some(0)));
        // `c` carries the bridge edge → highest degree.
        assert_eq!(all[0].chunk_id, "c");
        assert_eq!(all[0].identifier, "fn_c");

        let limited = central_nodes("p", &calls, &[], &assignments, &members, 1);
        assert_eq!(limited.len(), 1);
    }

    #[test]
    fn drift_wraps_compare() {
        let members: HashMap<String, CodeChunk> =
            [("a".to_string(), code("a", "p"))].into_iter().collect();
        let report = drift("p", &[assignment("a", 0)], &members);
        assert_eq!(report.communities.len(), 1);
        assert_eq!(report.mean_purity, 1.0);
    }

    #[test]
    fn cycles_reexport_is_callable() {
        assert!(find_import_cycles(&[]).is_empty());
    }
}
