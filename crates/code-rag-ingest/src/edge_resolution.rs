//! Call edge resolution: resolve ephemeral call identifiers to persistent CallEdge records.
//!
//! Post-ingestion step: takes chunks + calls_map + imports_map, produces Vec<CallEdge>.
//! Three-tier resolution with short-circuit at first unambiguous match.

use std::collections::HashMap;

use code_rag_types::{CallEdge, content_hash};

use crate::ingestion::language::ImportInfo;

/// Resolve call identifiers to CallEdge records using tiered disambiguation.
///
/// Tiers (in priority order, short-circuits at first unique match):
/// 1. Same-file: callee identifier found in the same file's chunk list
/// 2. Import-based: callee identifier matches an import → resolve source path to file
/// 3. Unique-global: only one chunk with that identifier across the entire project
///
/// Ambiguous calls (multiple candidates, no import evidence) are skipped.
pub fn resolve_edges(
    chunks: &[code_rag_types::CodeChunk],
    calls_map: &HashMap<String, Vec<String>>,
    imports_by_file: &HashMap<String, Vec<ImportInfo>>,
) -> Vec<CallEdge> {
    // Build identifier → [(chunk_id, file_path)] index
    let mut id_index: HashMap<&str, Vec<(&str, &str)>> = HashMap::new();
    for chunk in chunks {
        id_index
            .entry(chunk.identifier.as_str())
            .or_default()
            .push((chunk.chunk_id.as_str(), chunk.file_path.as_str()));
    }

    // Build chunk_id → chunk lookup
    let chunk_by_id: HashMap<&str, &code_rag_types::CodeChunk> =
        chunks.iter().map(|c| (c.chunk_id.as_str(), c)).collect();

    // Build file_path → import source_path → imported_names for tier 2
    // Maps: file → { imported_name → source_path }
    let mut import_lookup: HashMap<&str, HashMap<&str, &str>> = HashMap::new();
    for (file, file_imports) in imports_by_file {
        let entry = import_lookup.entry(file.as_str()).or_default();
        for imp in file_imports {
            entry.insert(imp.imported_name.as_str(), imp.source_path.as_str());
        }
    }

    let mut edges = Vec::new();

    for (caller_chunk_id, callee_identifiers) in calls_map {
        let caller = match chunk_by_id.get(caller_chunk_id.as_str()) {
            Some(c) => c,
            None => continue,
        };

        for callee_id in callee_identifiers {
            let candidates = match id_index.get(callee_id.as_str()) {
                Some(c) => c,
                None => continue, // No chunk with this identifier exists
            };

            // Skip self-edges (function calling itself)
            let non_self: Vec<_> = candidates
                .iter()
                .filter(|(cid, _)| *cid != caller_chunk_id.as_str())
                .collect();

            if non_self.is_empty() {
                continue;
            }

            // Tier 1: same-file match
            let same_file: Vec<_> = non_self
                .iter()
                .filter(|(_, fp)| *fp == caller.file_path.as_str())
                .collect();

            if same_file.len() == 1 {
                edges.push(make_edge(
                    caller,
                    same_file[0].0,
                    callee_id,
                    same_file[0].1,
                    1,
                ));
                continue;
            }

            // Tier 2: import-based match
            if let Some(file_imports) = import_lookup.get(caller.file_path.as_str())
                && let Some(source_path) = file_imports.get(callee_id.as_str())
            {
                // Find candidates whose file_path matches the resolved import source
                let import_matches: Vec<_> = non_self
                    .iter()
                    .filter(|(_, fp)| path_matches_import(fp, source_path))
                    .collect();

                if import_matches.len() == 1 {
                    edges.push(make_edge(
                        caller,
                        import_matches[0].0,
                        callee_id,
                        import_matches[0].1,
                        2,
                    ));
                    continue;
                }
            }

            // Tier 3: unique-global match
            if non_self.len() == 1 {
                edges.push(make_edge(
                    caller,
                    non_self[0].0,
                    callee_id,
                    non_self[0].1,
                    3,
                ));
                continue;
            }

            // Ambiguous: skip
        }
    }

    edges
}

/// Check if a file path matches a Rust import source path.
/// E.g., `source_path = "crate::ingestion::parser"` should match
/// `file_path = "src/ingestion/parser.rs"` or `"src/ingestion/parser/mod.rs"`.
fn path_matches_import(file_path: &str, source_path: &str) -> bool {
    // Strip crate prefix and convert :: to /
    let normalized = source_path
        .trim_start_matches("crate::")
        .trim_start_matches("super::")
        .trim_start_matches("self::")
        .replace("::", "/")
        .replace('.', "/"); // Python dots

    // Check if the file path ends with the normalized import path
    // Rust: src/module.rs or src/module/mod.rs
    // Python: module.py or module/__init__.py
    let checks = [
        format!("{}.rs", normalized),
        format!("{}/mod.rs", normalized),
        format!("{}.py", normalized),
        format!("{}/__init__.py", normalized),
        format!("{}.ts", normalized),
        format!("{}.tsx", normalized),
        format!("{}/index.ts", normalized),
        format!("{}/index.tsx", normalized),
    ];

    for check in &checks {
        if file_path.ends_with(check.as_str()) || file_path == check.as_str() {
            return true;
        }
    }

    // Also check if import path directly matches file path (without extension mapping)
    file_path.contains(&normalized)
}

fn make_edge(
    caller: &code_rag_types::CodeChunk,
    callee_chunk_id: &str,
    callee_identifier: &str,
    callee_file: &str,
    tier: u8,
) -> CallEdge {
    CallEdge {
        edge_id: content_hash(&format!("edge:{}:{}", caller.chunk_id, callee_chunk_id)),
        caller_chunk_id: caller.chunk_id.clone(),
        callee_chunk_id: callee_chunk_id.to_string(),
        caller_identifier: caller.identifier.clone(),
        callee_identifier: callee_identifier.to_string(),
        caller_file: caller.file_path.clone(),
        callee_file: callee_file.to_string(),
        project_name: caller.project_name.clone(),
        resolution_tier: tier,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use code_rag_types::CodeChunk;

    fn make_chunk(chunk_id: &str, identifier: &str, file_path: &str) -> CodeChunk {
        CodeChunk {
            file_path: file_path.into(),
            language: "rust".into(),
            identifier: identifier.into(),
            node_type: "function_item".into(),
            code_content: format!("fn {}() {{}}", identifier),
            start_line: 1,
            project_name: "test".into(),
            docstring: None,
            signature: None,
            chunk_id: chunk_id.into(),
            content_hash: "hash".into(),
            embedding_model_version: "test".into(),
        }
    }

    #[test]
    fn test_same_file_resolution() {
        let chunks = vec![
            make_chunk("c_foo", "foo", "src/lib.rs"),
            make_chunk("c_bar", "bar", "src/lib.rs"),
        ];
        let mut calls_map = HashMap::new();
        calls_map.insert("c_foo".into(), vec!["bar".into()]);

        let edges = resolve_edges(&chunks, &calls_map, &HashMap::new());
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].caller_chunk_id, "c_foo");
        assert_eq!(edges[0].callee_chunk_id, "c_bar");
        assert_eq!(edges[0].resolution_tier, 1);
    }

    #[test]
    fn test_unique_global_resolution() {
        let chunks = vec![
            make_chunk("c_foo", "foo", "src/a.rs"),
            make_chunk("c_bar", "bar", "src/b.rs"),
        ];
        let mut calls_map = HashMap::new();
        calls_map.insert("c_foo".into(), vec!["bar".into()]);

        let edges = resolve_edges(&chunks, &calls_map, &HashMap::new());
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].resolution_tier, 3);
    }

    #[test]
    fn test_same_file_wins_over_global() {
        // bar exists in both src/a.rs and src/b.rs, but foo is in src/a.rs
        let chunks = vec![
            make_chunk("c_foo", "foo", "src/a.rs"),
            make_chunk("c_bar1", "bar", "src/a.rs"),
            make_chunk("c_bar2", "bar", "src/b.rs"),
        ];
        let mut calls_map = HashMap::new();
        calls_map.insert("c_foo".into(), vec!["bar".into()]);

        let edges = resolve_edges(&chunks, &calls_map, &HashMap::new());
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].callee_chunk_id, "c_bar1"); // same-file wins
        assert_eq!(edges[0].resolution_tier, 1);
    }

    #[test]
    fn test_ambiguous_skipped() {
        // bar exists in two other files, no same-file match, no import
        let chunks = vec![
            make_chunk("c_foo", "foo", "src/a.rs"),
            make_chunk("c_bar1", "bar", "src/b.rs"),
            make_chunk("c_bar2", "bar", "src/c.rs"),
        ];
        let mut calls_map = HashMap::new();
        calls_map.insert("c_foo".into(), vec!["bar".into()]);

        let edges = resolve_edges(&chunks, &calls_map, &HashMap::new());
        assert!(edges.is_empty()); // ambiguous, skip
    }

    #[test]
    fn test_self_call_skipped() {
        let chunks = vec![make_chunk("c_foo", "foo", "src/lib.rs")];
        let mut calls_map = HashMap::new();
        calls_map.insert("c_foo".into(), vec!["foo".into()]);

        let edges = resolve_edges(&chunks, &calls_map, &HashMap::new());
        assert!(edges.is_empty());
    }

    #[test]
    fn test_unknown_callee_skipped() {
        let chunks = vec![make_chunk("c_foo", "foo", "src/lib.rs")];
        let mut calls_map = HashMap::new();
        calls_map.insert("c_foo".into(), vec!["nonexistent".into()]);

        let edges = resolve_edges(&chunks, &calls_map, &HashMap::new());
        assert!(edges.is_empty());
    }

    #[test]
    fn test_import_based_resolution() {
        let chunks = vec![
            make_chunk("c_foo", "foo", "src/a.rs"),
            make_chunk("c_bar1", "bar", "src/module/b.rs"),
            make_chunk("c_bar2", "bar", "src/other/c.rs"),
        ];
        let mut calls_map = HashMap::new();
        calls_map.insert("c_foo".into(), vec!["bar".into()]);

        let mut imports_map = HashMap::new();
        imports_map.insert(
            "src/a.rs".into(),
            vec![ImportInfo {
                imported_name: "bar".into(),
                source_path: "crate::module::b".into(),
            }],
        );

        let edges = resolve_edges(&chunks, &calls_map, &imports_map);
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].callee_chunk_id, "c_bar1");
        assert_eq!(edges[0].resolution_tier, 2);
    }

    #[test]
    fn test_deterministic_edge_id() {
        let chunks = vec![
            make_chunk("c_foo", "foo", "src/lib.rs"),
            make_chunk("c_bar", "bar", "src/lib.rs"),
        ];
        let mut calls_map = HashMap::new();
        calls_map.insert("c_foo".into(), vec!["bar".into()]);

        let edges1 = resolve_edges(&chunks, &calls_map, &HashMap::new());
        let edges2 = resolve_edges(&chunks, &calls_map, &HashMap::new());
        assert_eq!(edges1[0].edge_id, edges2[0].edge_id);
    }

    #[test]
    fn test_path_matches_import_rust() {
        assert!(path_matches_import(
            "src/ingestion/parser.rs",
            "crate::ingestion::parser"
        ));
        assert!(path_matches_import(
            "src/ingestion/parser/mod.rs",
            "crate::ingestion::parser"
        ));
        assert!(!path_matches_import(
            "src/other/parser.rs",
            "crate::ingestion::parser"
        ));
    }

    #[test]
    fn test_path_matches_import_python() {
        assert!(path_matches_import("utils/helper.py", "utils.helper"));
        assert!(!path_matches_import("other/helper.py", "utils.helper"));
    }
}
