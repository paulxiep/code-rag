//! R5: pure response shaping for the topology tools (`code_rag_communities`,
//! `code_rag_central_nodes`, `code_rag_cycles`, `code_rag_path`).
//!
//! Handlers in `main.rs` fetch persisted rows through the store seam and call
//! `code-raptor::insights` for the math; everything JSON-shaped lives here so
//! it is unit-testable without the RPC registry.

use code_rag_engine::mermaid::PathStep;
use code_rag_types::ClusterChunk;
use code_raptor::insights::{CentralEntry, Cycle, Divergence, DriftReport};
use serde_json::{Value, json};

/// Resolve the optional `project` param against the indexed projects.
///
/// - explicit + indexed → just that project
/// - explicit + unknown → `Err` naming what is available
/// - absent → every indexed project (the caller emits one entry per project;
///   with a single indexed project that collapses to the object shape)
pub fn resolve_projects(
    requested: Option<&str>,
    available: &[String],
) -> Result<Vec<String>, String> {
    match requested.map(str::trim).filter(|s| !s.is_empty()) {
        Some(p) if available.iter().any(|a| a == p) => Ok(vec![p.to_string()]),
        Some(p) => Err(format!(
            "unknown project '{p}' — indexed projects: {}",
            available.join(", ")
        )),
        None => Ok(available.to_vec()),
    }
}

/// Wrap per-project payloads: one project → the bare object; several → an
/// array under `projects`.
pub fn per_project_body(mut payloads: Vec<Value>) -> Value {
    if payloads.len() == 1 {
        payloads.remove(0)
    } else {
        json!({ "projects": payloads })
    }
}

/// One project's emergent modules + drift, from the persisted cluster rows.
pub fn communities_response(
    project: &str,
    cluster_chunks: &[ClusterChunk],
    drift: &DriftReport,
) -> Value {
    let communities: Vec<Value> = cluster_chunks
        .iter()
        .map(|c| {
            json!({
                "id": c.cluster_id,
                "size": c.member_chunk_ids.len(),
                "cohesion": c.cohesion,
                "directory": c.path,
                "key_functions": c.key_functions,
                "key_types": c.key_types,
                "files": c.files,
                "summary": c.summary_text,
            })
        })
        .collect();
    json!({
        "project": project,
        "community_count": communities.len(),
        "communities": communities,
        "drift": drift_json(drift),
    })
}

fn drift_json(drift: &DriftReport) -> Value {
    let divergences: Vec<Value> = drift
        .divergences
        .iter()
        .map(|d| match d {
            Divergence::ScatteredCommunity(c) => json!({
                "kind": "scattered_community",
                "community": c.community_id,
                "size": c.size,
                "dominant_directory": c.dominant_dir,
                "purity": c.purity,
                "spans_directories": c.dirs,
            }),
            Divergence::FragmentedFolder(f) => json!({
                "kind": "fragmented_folder",
                "directory": f.dir,
                "size": f.size,
                "dominant_community": f.dominant_community,
                "concentration": f.concentration,
                "splits_into_communities": f.communities,
            }),
        })
        .collect();
    json!({
        "mean_purity": drift.mean_purity,
        "note": "purity = share of a community's members living in its dominant directory; \
                 low purity or fragmented folders indicate the emergent architecture \
                 diverges from the folder layout",
        "divergences": divergences,
    })
}

/// One project's "read these first" list.
pub fn central_nodes_response(project: &str, entries: &[CentralEntry]) -> Value {
    let nodes: Vec<Value> = entries
        .iter()
        .map(|e| {
            json!({
                "identifier": e.identifier,
                "file": e.file,
                "chunk_id": e.chunk_id,
                "degree": e.degree,
                "community": e.community_id,
            })
        })
        .collect();
    json!({ "project": project, "nodes": nodes })
}

/// One project's import cycles; an empty list is stated as a positive signal.
pub fn cycles_response(project: &str, cycles: &[Cycle]) -> Value {
    let list: Vec<Value> = cycles.iter().map(|c| json!({ "files": c.files })).collect();
    let mut body = json!({
        "project": project,
        "cycle_count": list.len(),
        "cycles": list,
    });
    if list.is_empty() {
        body["note"] = json!("no circular dependencies — the file-level import graph is acyclic");
    }
    body
}

/// A found call path (+ its Mermaid rendering) or an explicit not-found.
pub fn path_response(
    from: &str,
    to: &str,
    steps: Option<(&[PathStep], &str)>,
    mermaid: &str,
) -> Value {
    match steps {
        Some((steps, direction)) => {
            let hops: Vec<Value> = steps
                .iter()
                .map(|s| json!({ "identifier": s.label, "file": s.file, "chunk_id": s.id }))
                .collect();
            json!({
                "from": from,
                "to": to,
                "found": true,
                "direction": direction,
                "hops": hops.len().saturating_sub(1),
                "steps": hops,
                "mermaid": mermaid,
            })
        }
        None => json!({
            "from": from,
            "to": to,
            "found": false,
            "note": "both identifiers resolved but no call chain connects them in either direction",
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use code_raptor::insights::CommunityDrift;

    #[test]
    fn resolve_explicit_unknown_and_absent() {
        let available = vec!["a".to_string(), "b".to_string()];
        assert_eq!(resolve_projects(Some("a"), &available).unwrap(), vec!["a"]);
        assert!(
            resolve_projects(Some("zzz"), &available)
                .unwrap_err()
                .contains("a, b")
        );
        assert_eq!(resolve_projects(None, &available).unwrap(), available);
        assert_eq!(resolve_projects(Some("  "), &available).unwrap(), available);
    }

    #[test]
    fn per_project_collapses_singletons() {
        let one = per_project_body(vec![json!({"project": "a"})]);
        assert_eq!(one["project"], "a");
        let many = per_project_body(vec![json!({"project": "a"}), json!({"project": "b"})]);
        assert_eq!(many["projects"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn communities_shape_and_drift() {
        let cc = ClusterChunk {
            cluster_id: 0,
            project_name: "p".into(),
            path: "p/src".into(),
            member_chunk_ids: vec!["a".into(), "b".into()],
            files: vec!["p/src/x.rs".into()],
            key_types: vec!["T".into()],
            key_functions: vec!["run".into()],
            dominant_relation: "calls".into(),
            cohesion: 0.4,
            summary_text: "Cluster 0".into(),
            chunk_id: "cluster:p#0".into(),
            content_hash: "h".into(),
            embedding_model_version: "test".into(),
        };
        let drift = DriftReport {
            communities: vec![],
            folders: vec![],
            divergences: vec![Divergence::ScatteredCommunity(CommunityDrift {
                community_id: 0,
                size: 4,
                dominant_dir: "p/src".into(),
                purity: 0.5,
                dirs: vec!["p/src".into(), "p/other".into()],
            })],
            mean_purity: 0.5,
        };
        let body = communities_response("p", &[cc], &drift);
        assert_eq!(body["community_count"], 1);
        assert_eq!(body["communities"][0]["size"], 2);
        assert_eq!(
            body["drift"]["divergences"][0]["kind"],
            "scattered_community"
        );
    }

    #[test]
    fn cycles_empty_is_positive_note() {
        let body = cycles_response("p", &[]);
        assert_eq!(body["cycle_count"], 0);
        assert!(body["note"].as_str().unwrap().contains("acyclic"));
        let body = cycles_response(
            "p",
            &[Cycle {
                files: vec!["a.rs".into(), "b.rs".into()],
            }],
        );
        assert_eq!(body["cycle_count"], 1);
        assert!(body.get("note").is_none());
    }

    #[test]
    fn path_found_and_not_found() {
        let steps = vec![
            PathStep {
                id: "c1".into(),
                label: "a".into(),
                file: "x.rs".into(),
            },
            PathStep {
                id: "c2".into(),
                label: "b".into(),
                file: "y.rs".into(),
            },
        ];
        let body = path_response("a", "b", Some((&steps, "forward")), "flowchart LR\n...");
        assert_eq!(body["found"], true);
        assert_eq!(body["hops"], 1);
        assert_eq!(body["steps"][1]["identifier"], "b");
        let none = path_response("a", "b", None, "");
        assert_eq!(none["found"], false);
    }
}
