//! Call edge resolution: resolve ephemeral call identifiers to persistent CallEdge records.
//!
//! Post-ingestion step: takes chunks + calls_map + imports_map, produces Vec<CallEdge>.
//! Three-tier resolution with short-circuit at first unambiguous match.

use std::collections::HashMap;

use code_rag_types::{
    CallEdge, CodeChunk, EdgeConfidence, EdgeContext, EdgeRelation, FileChunk, FolderChunk,
    GraphEdge, content_hash,
};

use crate::ingestion::language::{ImportInfo, TypeRelation};

/// Last path segment of a normalized (forward-slash) path.
fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Parent directory of a normalized (forward-slash) path, or None at the root.
fn parent_dir(path: &str) -> Option<&str> {
    path.rfind('/').map(|i| &path[..i])
}

/// Build a `GraphEdge` for relations whose endpoints are already known chunk ids
/// (no identifier resolution needed) — e.g. `Contains`.
fn direct_edge(
    relation: EdgeRelation,
    source: (&str, &str, &str),
    target: (&str, &str, &str),
    project: &str,
) -> GraphEdge {
    let (source_id, source_ident, source_file) = source;
    let (target_id, target_ident, target_file) = target;
    GraphEdge {
        edge_id: GraphEdge::deterministic_edge_id(
            source_id,
            target_id,
            relation,
            EdgeContext::None,
        ),
        source_chunk_id: source_id.to_string(),
        target_chunk_id: target_id.to_string(),
        source_identifier: source_ident.to_string(),
        target_identifier: target_ident.to_string(),
        source_file: source_file.to_string(),
        target_file: target_file.to_string(),
        project_name: project.to_string(),
        relation,
        context: EdgeContext::None,
        confidence: EdgeConfidence::Extracted,
    }
}

/// Track R (R1): derive `Contains` edges from the chunk hierarchy — folder ⊇ file
/// ⊇ definition. No parsing; endpoints are the existing folder/file/code chunk ids.
/// These feed the R2 community-detection union (`contains` is one of its relations).
pub fn build_contains_edges(
    code_chunks: &[CodeChunk],
    file_chunks: &[FileChunk],
    folder_chunks: &[FolderChunk],
) -> Vec<GraphEdge> {
    let mut edges = Vec::new();
    let file_by_path: HashMap<&str, &FileChunk> = file_chunks
        .iter()
        .map(|f| (f.file_path.as_str(), f))
        .collect();
    let folder_by_path: HashMap<&str, &FolderChunk> = folder_chunks
        .iter()
        .map(|f| (f.folder_path.as_str(), f))
        .collect();

    // file ⊇ definition
    for code in code_chunks {
        if let Some(fc) = file_by_path.get(code.file_path.as_str()) {
            edges.push(direct_edge(
                EdgeRelation::Contains,
                (&fc.chunk_id, basename(&fc.file_path), &fc.file_path),
                (&code.chunk_id, &code.identifier, &code.file_path),
                &code.project_name,
            ));
        }
    }

    // folder ⊇ file
    for fc in file_chunks {
        if let Some(parent) = parent_dir(&fc.file_path)
            && let Some(folder) = folder_by_path.get(parent)
        {
            edges.push(direct_edge(
                EdgeRelation::Contains,
                (
                    &folder.chunk_id,
                    basename(&folder.folder_path),
                    &folder.folder_path,
                ),
                (&fc.chunk_id, basename(&fc.file_path), &fc.file_path),
                &fc.project_name,
            ));
        }
    }

    edges
}

/// Track R (R1): resolve file-level imports into `Imports` / `ReExports` graph
/// edges. The edge source is the *importing file's* FileChunk; the target is the
/// resolved imported symbol's chunk (same tiers as call/type resolution). A
/// `pub use` / `export … from` is emitted as `ReExports`, everything else as
/// `Imports`. Imports whose symbol resolves to no project chunk are dropped.
pub fn build_import_edges(
    code_chunks: &[CodeChunk],
    file_chunks: &[FileChunk],
    imports_by_file: &HashMap<String, Vec<ImportInfo>>,
) -> Vec<GraphEdge> {
    let mut id_index: HashMap<&str, Vec<(&str, &str)>> = HashMap::new();
    for chunk in code_chunks {
        id_index
            .entry(chunk.identifier.as_str())
            .or_default()
            .push((chunk.chunk_id.as_str(), chunk.file_path.as_str()));
    }
    let mut import_lookup: HashMap<&str, HashMap<&str, &str>> = HashMap::new();
    for (file, file_imports) in imports_by_file {
        let entry = import_lookup.entry(file.as_str()).or_default();
        for imp in file_imports {
            entry.insert(imp.imported_name.as_str(), imp.source_path.as_str());
        }
    }
    let file_by_path: HashMap<&str, &FileChunk> = file_chunks
        .iter()
        .map(|f| (f.file_path.as_str(), f))
        .collect();

    let mut edges = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for (file, imports) in imports_by_file {
        let src = match file_by_path.get(file.as_str()) {
            Some(f) => f,
            None => continue, // file produced no FileChunk (e.g. no definitions)
        };
        for imp in imports {
            if let Some((tid, tfile, tier)) = resolve_target(
                &src.chunk_id,
                file,
                &imp.imported_name,
                &id_index,
                &import_lookup,
            ) {
                let relation = if imp.is_reexport {
                    EdgeRelation::ReExports
                } else {
                    EdgeRelation::Imports
                };
                let edge_id = GraphEdge::deterministic_edge_id(
                    &src.chunk_id,
                    tid,
                    relation,
                    EdgeContext::None,
                );
                if !seen.insert(edge_id.clone()) {
                    continue;
                }
                edges.push(GraphEdge {
                    edge_id,
                    source_chunk_id: src.chunk_id.clone(),
                    target_chunk_id: tid.to_string(),
                    source_identifier: basename(file).to_string(),
                    target_identifier: imp.imported_name.clone(),
                    source_file: file.clone(),
                    target_file: tfile.to_string(),
                    project_name: src.project_name.clone(),
                    relation,
                    context: EdgeContext::None,
                    confidence: if tier <= 2 {
                        EdgeConfidence::Extracted
                    } else {
                        EdgeConfidence::Inferred
                    },
                });
            }
        }
    }
    edges
}

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

/// Track R (R1): resolve extracted type relations to persistent `GraphEdge`s.
///
/// Reuses the same tiered disambiguation as call resolution (same-file > import >
/// unique-global). `type_relations` maps `source_chunk_id → [TypeRelation]`. A
/// relation whose target identifier matches no project chunk (e.g. `Vec`, `String`,
/// or a third-party type) is dropped — only intra-project structural edges survive.
/// Confidence: tier 1/2 (same-file / import) → `Extracted`; tier 3 (unique-global)
/// → `Inferred`; ambiguous/self → skipped.
pub fn resolve_type_edges(
    chunks: &[code_rag_types::CodeChunk],
    type_relations: &HashMap<String, Vec<TypeRelation>>,
    imports_by_file: &HashMap<String, Vec<ImportInfo>>,
) -> Vec<GraphEdge> {
    let mut id_index: HashMap<&str, Vec<(&str, &str)>> = HashMap::new();
    for chunk in chunks {
        id_index
            .entry(chunk.identifier.as_str())
            .or_default()
            .push((chunk.chunk_id.as_str(), chunk.file_path.as_str()));
    }
    let chunk_by_id: HashMap<&str, &code_rag_types::CodeChunk> =
        chunks.iter().map(|c| (c.chunk_id.as_str(), c)).collect();

    let mut import_lookup: HashMap<&str, HashMap<&str, &str>> = HashMap::new();
    for (file, file_imports) in imports_by_file {
        let entry = import_lookup.entry(file.as_str()).or_default();
        for imp in file_imports {
            entry.insert(imp.imported_name.as_str(), imp.source_path.as_str());
        }
    }

    let mut edges = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    for (source_chunk_id, relations) in type_relations {
        let source = match chunk_by_id.get(source_chunk_id.as_str()) {
            Some(c) => c,
            None => continue, // source chunk was filtered out (e.g. test module)
        };

        for rel in relations {
            if let Some((target_chunk_id, target_file, tier)) = resolve_target(
                source.chunk_id.as_str(),
                source.file_path.as_str(),
                &rel.target_name,
                &id_index,
                &import_lookup,
            ) {
                let confidence = if tier <= 2 {
                    EdgeConfidence::Extracted
                } else {
                    EdgeConfidence::Inferred
                };
                let edge_id = GraphEdge::deterministic_edge_id(
                    &source.chunk_id,
                    target_chunk_id,
                    rel.relation,
                    rel.context,
                );
                // Collapse duplicate (source,target,relation,context) tuples that
                // can arise from repeated captures of the same definition.
                if !seen.insert(edge_id.clone()) {
                    continue;
                }
                edges.push(GraphEdge {
                    edge_id,
                    source_chunk_id: source.chunk_id.clone(),
                    target_chunk_id: target_chunk_id.to_string(),
                    source_identifier: source.identifier.clone(),
                    target_identifier: rel.target_name.clone(),
                    source_file: source.file_path.clone(),
                    target_file: target_file.to_string(),
                    project_name: source.project_name.clone(),
                    relation: rel.relation,
                    context: rel.context,
                    confidence,
                });
            }
        }
    }

    edges
}

/// Resolve a referenced identifier to a single chunk using the same tiers as call
/// resolution. Returns `(target_chunk_id, target_file, tier)` or `None` for
/// unknown/self/ambiguous targets. Shared by call and type-relation resolution.
fn resolve_target<'a>(
    source_chunk_id: &str,
    source_file: &str,
    target_name: &str,
    id_index: &HashMap<&'a str, Vec<(&'a str, &'a str)>>,
    import_lookup: &HashMap<&str, HashMap<&str, &str>>,
) -> Option<(&'a str, &'a str, u8)> {
    let candidates = id_index.get(target_name)?;
    let non_self: Vec<_> = candidates
        .iter()
        .filter(|(cid, _)| *cid != source_chunk_id)
        .collect();
    if non_self.is_empty() {
        return None;
    }

    // Tier 1: same-file
    let same_file: Vec<_> = non_self
        .iter()
        .filter(|(_, fp)| *fp == source_file)
        .collect();
    if same_file.len() == 1 {
        return Some((same_file[0].0, same_file[0].1, 1));
    }

    // Tier 2: import-based
    if let Some(file_imports) = import_lookup.get(source_file)
        && let Some(src_path) = file_imports.get(target_name)
    {
        let import_matches: Vec<_> = non_self
            .iter()
            .filter(|(_, fp)| path_matches_import(fp, src_path))
            .collect();
        if import_matches.len() == 1 {
            return Some((import_matches[0].0, import_matches[0].1, 2));
        }
    }

    // Tier 3: unique-global
    if non_self.len() == 1 {
        return Some((non_self[0].0, non_self[0].1, 3));
    }

    None // ambiguous
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
            vec![ImportInfo::import("bar", "crate::module::b")],
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

    // ---- Track R (R1): type-relation resolution ----

    #[test]
    fn test_resolve_type_edges_implements() {
        use code_rag_types::{EdgeContext, EdgeRelation};
        // FastEmbedImpl implements the Embedder trait; both are project chunks.
        let chunks = vec![
            make_chunk("c_fe", "FastEmbedImpl", "src/embedder.rs"),
            make_chunk("c_emb", "Embedder", "src/seams.rs"),
        ];
        let mut rels = HashMap::new();
        rels.insert(
            "c_fe".to_string(),
            vec![TypeRelation::new(
                "Embedder",
                EdgeRelation::Implements,
                EdgeContext::None,
            )],
        );
        let edges = resolve_type_edges(&chunks, &rels, &HashMap::new());
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].source_chunk_id, "c_fe");
        assert_eq!(edges[0].target_chunk_id, "c_emb");
        assert_eq!(edges[0].relation, EdgeRelation::Implements);
        // unique-global resolution → Inferred confidence
        assert_eq!(
            edges[0].confidence,
            code_rag_types::EdgeConfidence::Inferred
        );
    }

    #[test]
    fn test_resolve_type_edges_drops_unknown_target() {
        use code_rag_types::{EdgeContext, EdgeRelation};
        // `Vec` is not a project chunk → the reference is dropped.
        let chunks = vec![make_chunk("c_fe", "FastEmbedImpl", "src/embedder.rs")];
        let mut rels = HashMap::new();
        rels.insert(
            "c_fe".to_string(),
            vec![TypeRelation::new(
                "Vec",
                EdgeRelation::References,
                EdgeContext::ReturnType,
            )],
        );
        let edges = resolve_type_edges(&chunks, &rels, &HashMap::new());
        assert!(edges.is_empty());
    }

    #[test]
    fn test_resolve_type_edges_same_file_extracted() {
        use code_rag_types::{EdgeConfidence, EdgeContext, EdgeRelation};
        // Same-file resolution → Extracted confidence.
        let chunks = vec![
            make_chunk("c_a", "Foo", "src/lib.rs"),
            make_chunk("c_b", "Bar", "src/lib.rs"),
        ];
        let mut rels = HashMap::new();
        rels.insert(
            "c_a".to_string(),
            vec![TypeRelation::new(
                "Bar",
                EdgeRelation::Embeds,
                EdgeContext::None,
            )],
        );
        let edges = resolve_type_edges(&chunks, &rels, &HashMap::new());
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].confidence, EdgeConfidence::Extracted);
    }

    // ---- Track R (R1b): contains + imports ----

    fn make_file_chunk(chunk_id: &str, file_path: &str) -> FileChunk {
        FileChunk {
            file_path: file_path.into(),
            project_name: "test".into(),
            language: "rust".into(),
            exports: vec![],
            imports: vec![],
            purpose: None,
            summary_text: String::new(),
            chunk_id: chunk_id.into(),
            content_hash: "h".into(),
            embedding_model_version: "v".into(),
        }
    }

    fn make_folder_chunk(chunk_id: &str, folder_path: &str) -> FolderChunk {
        FolderChunk {
            folder_path: folder_path.into(),
            project_name: "test".into(),
            file_count: 1,
            languages: vec![],
            key_types: vec![],
            key_functions: vec![],
            subfolders: vec![],
            summary_text: String::new(),
            chunk_id: chunk_id.into(),
            content_hash: "h".into(),
            embedding_model_version: "v".into(),
        }
    }

    #[test]
    fn test_build_contains_edges() {
        use code_rag_types::EdgeRelation;
        let code = vec![make_chunk("c_fn", "foo", "p/src/lib.rs")];
        let files = vec![make_file_chunk("c_file", "p/src/lib.rs")];
        let folders = vec![make_folder_chunk("c_folder", "p/src")];
        let edges = build_contains_edges(&code, &files, &folders);
        // file ⊇ def
        assert!(edges.iter().any(|e| e.relation == EdgeRelation::Contains
            && e.source_chunk_id == "c_file"
            && e.target_chunk_id == "c_fn"));
        // folder ⊇ file
        assert!(edges.iter().any(|e| e.relation == EdgeRelation::Contains
            && e.source_chunk_id == "c_folder"
            && e.target_chunk_id == "c_file"));
    }

    #[test]
    fn test_build_import_edges_and_reexports() {
        use code_rag_types::EdgeRelation;
        // Bar is defined in mod_b.rs; foo.rs imports it (private) and lib.rs re-exports it.
        let code = vec![
            make_chunk("c_bar", "Bar", "p/src/mod_b.rs"),
            make_chunk("c_foo", "foo", "p/src/foo.rs"),
        ];
        let files = vec![
            make_file_chunk("c_foo_file", "p/src/foo.rs"),
            make_file_chunk("c_lib_file", "p/src/lib.rs"),
        ];
        let mut imports = HashMap::new();
        imports.insert(
            "p/src/foo.rs".to_string(),
            vec![ImportInfo::import("Bar", "crate::mod_b")],
        );
        imports.insert(
            "p/src/lib.rs".to_string(),
            vec![ImportInfo {
                imported_name: "Bar".into(),
                source_path: "crate::mod_b".into(),
                is_reexport: true,
            }],
        );
        let edges = build_import_edges(&code, &files, &imports);
        assert!(edges.iter().any(|e| e.relation == EdgeRelation::Imports
            && e.source_chunk_id == "c_foo_file"
            && e.target_chunk_id == "c_bar"));
        assert!(edges.iter().any(|e| e.relation == EdgeRelation::ReExports
            && e.source_chunk_id == "c_lib_file"
            && e.target_chunk_id == "c_bar"));
    }
}
