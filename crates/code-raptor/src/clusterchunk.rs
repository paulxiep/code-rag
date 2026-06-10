//! R3: assemble `ClusterChunk` summaries from detected communities.
//!
//! Pure (no I/O): given the community assignments + the member code chunks +
//! the edges, render one deterministic template summary per community. `lib.rs`
//! fetches the members, embeds `summary_text`, and upserts. Mirrors the
//! `FolderChunk` builder, reusing `code_rag_engine`'s template + visibility
//! heuristic so server and WASM see identical summary bytes.

use std::collections::{BTreeSet, HashMap, HashSet};

use code_rag_engine::cluster::{self as csum, ClusterMeta, MAX_FILES};
use code_rag_engine::folder::{MAX_KEYS, is_public};
use code_rag_types::{
    CallEdge, ClusterChunk, CodeChunk, EdgeRelation, GraphEdge, content_hash,
    deterministic_chunk_id,
};

use crate::cluster::CommunityResult;
use crate::topology::Topology;

/// Must match `code-rag-ingest`'s `DEFAULT_EMBEDDING_MODEL` so cluster chunks
/// are versioned identically to folder/file chunks (BGE-small, 384-dim).
const EMBEDDING_MODEL_VERSION: &str = "BGESmallENV15_384";

fn is_type_node(node_type: &str) -> bool {
    matches!(
        node_type,
        "struct_item"
            | "enum_item"
            | "trait_item"
            | "class_definition"
            | "class_declaration"
            | "interface_declaration"
            | "type_alias_declaration"
            | "enum_declaration"
    )
}

fn is_function_node(node_type: &str) -> bool {
    matches!(
        node_type,
        "function_item"
            | "function_definition"
            | "function_declaration"
            | "method_definition"
            | "lexical_declaration"
    )
}

fn basename(path: &str) -> &str {
    path.rsplit(['/', '\\'])
        .find(|s| !s.is_empty())
        .unwrap_or(path)
}

/// The cluster's representative directory: the parent dir most of its members
/// live in. Tie → the longer (more specific) dir, then lexically smallest, so
/// it is deterministic. Used as the cluster's creditable `file_path` in
/// retrieval flatten (a subsystem query expecting that crate/dir is satisfied
/// when the cluster surfaces). Empty when there are no members.
fn dominant_dir(chunks: &[&CodeChunk]) -> String {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for c in chunks {
        let dir = c
            .file_path
            .rfind('/')
            .map(|i| &c.file_path[..i])
            .unwrap_or("");
        *counts.entry(dir).or_insert(0) += 1;
    }
    counts
        .into_iter()
        .max_by(|a, b| {
            a.1.cmp(&b.1)
                .then(a.0.len().cmp(&b.0.len()))
                .then(b.0.cmp(a.0))
        })
        .map(|(d, _)| d.to_string())
        .unwrap_or_default()
}

/// Build `ClusterChunk`s (without embeddings) for one project's communities.
/// Output is ordered by `cluster_id` for deterministic embedding/upsert.
pub fn build_cluster_chunks(
    project: &str,
    topo: &Topology,
    results: &[CommunityResult],
    members: &HashMap<String, CodeChunk>,
    call_edges: &[CallEdge],
    graph_edges: &[GraphEdge],
) -> Vec<ClusterChunk> {
    // chunk_id -> community id (for the dominant-relation tally).
    let community_of: HashMap<&str, u32> = results
        .iter()
        .map(|r| (r.chunk_id.as_str(), r.community_id))
        .collect();
    // chunk_id -> weighted degree (for the central-member pick).
    let degree_of: HashMap<&str, f64> = (0..topo.ids.len())
        .map(|i| (topo.ids[i].as_str(), topo.graph.degree(i)))
        .collect();

    // Group members + cohesion by community.
    let mut by_comm: HashMap<u32, Vec<&CommunityResult>> = HashMap::new();
    for r in results {
        by_comm.entry(r.community_id).or_default().push(r);
    }

    // Dominant-relation tally over intra-community edges (same keep-rules as the
    // topology: drop folder→file contains and projected calls).
    let same_comm = |a: &str, b: &str| -> Option<u32> {
        match (community_of.get(a), community_of.get(b)) {
            (Some(&ca), Some(&cb)) if ca == cb => Some(ca),
            _ => None,
        }
    };
    let mut rel_tally: HashMap<u32, HashMap<&'static str, usize>> = HashMap::new();
    for e in call_edges {
        if let Some(c) = same_comm(&e.caller_chunk_id, &e.callee_chunk_id) {
            *rel_tally.entry(c).or_default().entry("calls").or_insert(0) += 1;
        }
    }
    for e in graph_edges {
        if e.source_chunk_id == e.target_chunk_id || e.relation == EdgeRelation::Calls {
            continue;
        }
        if e.relation == EdgeRelation::Contains && e.source_file != e.target_file {
            continue; // folder→file containment — not a partition input
        }
        if let Some(c) = same_comm(&e.source_chunk_id, &e.target_chunk_id) {
            *rel_tally
                .entry(c)
                .or_default()
                .entry(e.relation.as_str())
                .or_insert(0) += 1;
        }
    }

    let mut comm_ids: Vec<u32> = by_comm.keys().copied().collect();
    comm_ids.sort_unstable();

    let mut out = Vec::with_capacity(comm_ids.len());
    for cid in comm_ids {
        let group = &by_comm[&cid];
        let cohesion = group.first().map(|r| r.cohesion).unwrap_or(0.0);

        let mut member_ids: Vec<String> = group.iter().map(|r| r.chunk_id.clone()).collect();
        member_ids.sort();

        let chunks: Vec<&CodeChunk> = member_ids.iter().filter_map(|id| members.get(id)).collect();

        // Files the members span (basenames, sorted by BTreeSet).
        let files_set: BTreeSet<String> = chunks
            .iter()
            .map(|c| basename(&c.file_path).to_string())
            .collect();
        let files: Vec<String> = files_set.into_iter().take(MAX_FILES).collect();
        let file_count = chunks
            .iter()
            .map(|c| c.file_path.as_str())
            .collect::<HashSet<_>>()
            .len();

        // Public key types / functions.
        let mut types: BTreeSet<String> = BTreeSet::new();
        let mut fns: BTreeSet<String> = BTreeSet::new();
        for c in &chunks {
            if !is_public(&c.language, c.signature.as_deref(), &c.identifier) {
                continue;
            }
            if is_type_node(&c.node_type) {
                types.insert(c.identifier.clone());
            } else if is_function_node(&c.node_type) {
                fns.insert(c.identifier.clone());
            }
        }
        let key_types: Vec<String> = types.into_iter().take(MAX_KEYS).collect();
        let key_functions: Vec<String> = fns.into_iter().take(MAX_KEYS).collect();

        // Dominant intra-community relation (tie → lexically smallest tag).
        let dominant_relation = rel_tally
            .get(&cid)
            .and_then(|m| {
                m.iter()
                    .max_by(|a, b| a.1.cmp(b.1).then_with(|| b.0.cmp(a.0)))
                    .map(|(k, _)| k.to_string())
            })
            .unwrap_or_default();

        // Central member: highest weighted degree; tie → smallest chunk_id
        // (member_ids is sorted, and we only replace on strictly-greater degree).
        let mut central_member = String::new();
        let mut best_deg = f64::NEG_INFINITY;
        for id in &member_ids {
            let d = degree_of.get(id.as_str()).copied().unwrap_or(0.0);
            if d > best_deg
                && let Some(c) = members.get(id)
            {
                best_deg = d;
                central_member = c.identifier.clone();
            }
        }

        let meta = ClusterMeta {
            cluster_id: cid,
            member_count: member_ids.len(),
            file_count,
            key_types: &key_types,
            key_functions: &key_functions,
            files: &files,
            dominant_relation: &dominant_relation,
            cohesion,
            central_member: &central_member,
        };
        let summary_text = csum::render_summary(&meta);
        let canonical = csum::canonical_tuple(&meta);
        let chunk_id = deterministic_chunk_id(&format!("cluster:{project}:{cid}"), &summary_text);

        out.push(ClusterChunk {
            cluster_id: cid,
            project_name: project.to_string(),
            path: dominant_dir(&chunks),
            member_chunk_ids: member_ids,
            files,
            key_types,
            key_functions,
            dominant_relation,
            cohesion,
            summary_text,
            chunk_id,
            content_hash: content_hash(&canonical),
            embedding_model_version: EMBEDDING_MODEL_VERSION.to_string(),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn code(id: &str, file: &str, ident: &str, node: &str, sig: &str) -> CodeChunk {
        CodeChunk {
            file_path: file.into(),
            language: "rust".into(),
            identifier: ident.into(),
            node_type: node.into(),
            code_content: "..".into(),
            start_line: 1,
            project_name: "p".into(),
            docstring: None,
            signature: Some(sig.into()),
            chunk_id: id.into(),
            content_hash: "h".into(),
            embedding_model_version: "test".into(),
        }
    }

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

    #[test]
    fn builds_one_chunk_per_community() {
        // Two communities: {a,b} and {c}.
        let results = vec![
            CommunityResult {
                chunk_id: "a".into(),
                community_id: 0,
                cohesion: 1.0,
            },
            CommunityResult {
                chunk_id: "b".into(),
                community_id: 0,
                cohesion: 1.0,
            },
            CommunityResult {
                chunk_id: "c".into(),
                community_id: 1,
                cohesion: 0.0,
            },
        ];
        let mut members = HashMap::new();
        members.insert(
            "a".to_string(),
            code("a", "src/x.rs", "Alpha", "struct_item", "pub struct Alpha"),
        );
        members.insert(
            "b".to_string(),
            code("b", "src/x.rs", "run", "function_item", "pub fn run()"),
        );
        members.insert(
            "c".to_string(),
            code("c", "src/y.rs", "Other", "struct_item", "pub struct Other"),
        );
        let calls = vec![call("a", "b")];
        let topo = Topology::build(&calls, &[]);

        let chunks = build_cluster_chunks("p", &topo, &results, &members, &calls, &[]);
        assert_eq!(chunks.len(), 2);
        let c0 = &chunks[0];
        assert_eq!(c0.cluster_id, 0);
        assert_eq!(c0.path, "src");
        assert_eq!(c0.member_chunk_ids, vec!["a".to_string(), "b".to_string()]);
        assert!(c0.key_types.contains(&"Alpha".to_string()));
        assert!(c0.key_functions.contains(&"run".to_string()));
        assert_eq!(c0.dominant_relation, "calls");
        assert!(c0.summary_text.contains("module/subsystem"));
        // Deterministic across runs.
        let again = build_cluster_chunks("p", &topo, &results, &members, &calls, &[]);
        assert_eq!(again[0].chunk_id, c0.chunk_id);
    }
}
