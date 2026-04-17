//! A2: Build FolderChunks for a repository.
//!
//! Walks the project tree once (reusing `should_skip` from the parent module
//! so blocklists don't drift), aggregates per-directory file counts, languages,
//! and direct-subfolder lists, then folds in public `key_types`/`key_functions`
//! from the already-produced `CodeChunk` vec. Template rendering + the
//! public-visibility heuristic live in `code_rag_engine::folder` so server
//! and WASM see identical behavior.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use code_rag_engine::folder::{self as fsum, FolderMeta, MAX_KEYS};
use code_rag_types::{CodeChunk, FolderChunk, content_hash, deterministic_chunk_id};
use walkdir::WalkDir;

use super::languages::handler_for_path;
use super::{DEFAULT_EMBEDDING_MODEL, normalize_path, should_skip};

/// Mutable accumulator per folder during the walk phase.
#[derive(Default, Debug)]
struct FolderAcc {
    file_count: usize,
    languages: BTreeSet<String>,
    subfolders: BTreeSet<String>,
}

/// Build FolderChunks for every non-empty, non-vendor folder inside `repo_path`.
///
/// `repo_path` is the project's own root (or, for the portfolio-root ingest,
/// the portfolio root). The produced `folder_path` is normalized relative to
/// `repo_path` — identical shape to how `CodeChunk.file_path` is produced,
/// so harness expectations written against `project/crates/...` prefixes
/// keep working.
///
/// `code_chunks` is consulted for `key_types` and `key_functions` — filtered
/// to those whose `file_path.starts_with(folder_path + '/')`, then reduced
/// by node_type + public-visibility heuristic, sorted alphabetically, and
/// capped at `MAX_KEYS`.
///
/// `project_name_override` mirrors the convention in `run_ingestion`.
pub fn build_folder_chunks(
    repo_path: &str,
    code_chunks: &[CodeChunk],
    project_name_override: Option<&str>,
) -> Vec<FolderChunk> {
    let repo_root = PathBuf::from(repo_path);
    let mut acc: BTreeMap<String, FolderAcc> = BTreeMap::new();

    // Pass 1: walk the tree, registering every visited directory and
    // aggregating file_count/languages/subfolders per-directory.
    for entry in WalkDir::new(&repo_root)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if should_skip(&entry, &repo_root) {
            continue;
        }
        let path = entry.path();
        if entry.file_type().is_dir() {
            let norm = normalize_path(path, &repo_root);
            // Root of repo normalizes to "" — keep it keyed as the canonical
            // empty string so children's parent lookups resolve cleanly.
            acc.entry(norm).or_default();
        } else if entry.file_type().is_file()
            && let Some(parent) = path.parent()
        {
            let parent_norm = normalize_path(parent, &repo_root);
            let e = acc.entry(parent_norm.clone()).or_default();
            // Count only source files (exclude READMEs, Cargo.toml,
            // binary assets). `handler_for_path` returning Some == source.
            if let Some(handler) = handler_for_path(path) {
                e.file_count += 1;
                e.languages.insert(handler.name().to_string());
            }
            // Register subfolder relation in the grandparent when the
            // parent dir has been visited (it always has).
            if let (Some(grand), Some(name)) =
                (parent.parent(), parent.file_name().and_then(|n| n.to_str()))
                && grand.starts_with(&repo_root)
            {
                // Only emit a subfolder edge when the grandparent is
                // inside repo_root (prevents leaking e.g. "portfolio"
                // up above the ingest root).
                let grand_norm = normalize_path(grand, &repo_root);
                acc.entry(grand_norm)
                    .or_default()
                    .subfolders
                    .insert(name.to_string());
            }
        }
    }

    // Pass 1b: register subfolder relations for directory-only entries
    // (directories with only sub-directories and no files register them
    // via the file loop; purely-hierarchy folders still need their parent
    // to list them as subfolders).
    let dir_names: Vec<String> = acc.keys().cloned().collect();
    for dir_path in &dir_names {
        if dir_path.is_empty() {
            continue;
        }
        if let Some(idx) = dir_path.rfind('/') {
            let parent = &dir_path[..idx];
            let name = &dir_path[idx + 1..];
            acc.entry(parent.to_string())
                .or_default()
                .subfolders
                .insert(name.to_string());
        } else {
            // Top-level subfolder under the repo root.
            let name = dir_path.clone();
            acc.entry(String::new())
                .or_default()
                .subfolders
                .insert(name);
        }
    }

    // Pass 2: aggregate public key_types/key_functions per folder by
    // bucketing code_chunks. Use BTreeSet so the iteration order is
    // alphabetical and deterministic across platforms.
    let mut types_by_folder: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut fns_by_folder: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for chunk in code_chunks {
        let Some(parent) = chunk_parent_folder(&chunk.file_path) else {
            continue;
        };
        if !fsum::is_public(
            &chunk.language,
            chunk.signature.as_deref(),
            &chunk.identifier,
        ) {
            continue;
        }
        if is_type_node(&chunk.node_type) {
            types_by_folder
                .entry(parent.to_string())
                .or_default()
                .insert(chunk.identifier.clone());
        } else if is_function_node(&chunk.node_type) {
            fns_by_folder
                .entry(parent.to_string())
                .or_default()
                .insert(chunk.identifier.clone());
        }
    }

    // Pass 3: produce FolderChunk per accumulated folder.
    let mut out = Vec::with_capacity(acc.len());
    for (folder_path, a) in acc {
        let languages: Vec<String> = a.languages.into_iter().collect();
        let subfolders: Vec<String> = a.subfolders.into_iter().collect();
        let key_types: Vec<String> = types_by_folder
            .get(&folder_path)
            .map(|s| s.iter().take(MAX_KEYS).cloned().collect())
            .unwrap_or_default();
        let key_functions: Vec<String> = fns_by_folder
            .get(&folder_path)
            .map(|s| s.iter().take(MAX_KEYS).cloned().collect())
            .unwrap_or_default();

        // Skip empty folders — no content to summarize.
        if a.file_count == 0
            && subfolders.is_empty()
            && key_types.is_empty()
            && key_functions.is_empty()
        {
            continue;
        }

        let project_name = project_name_for_folder(&folder_path, &repo_root, project_name_override);

        let meta = FolderMeta {
            folder_path: &folder_path,
            file_count: a.file_count,
            languages: &languages,
            key_types: &key_types,
            key_functions: &key_functions,
            subfolders: &subfolders,
        };
        let summary_text = fsum::render_summary(&meta);
        let canonical = fsum::canonical_tuple(&meta);

        out.push(FolderChunk {
            chunk_id: deterministic_chunk_id(&folder_path, &summary_text),
            content_hash: content_hash(&canonical),
            folder_path,
            project_name,
            file_count: a.file_count,
            languages,
            key_types,
            key_functions,
            subfolders,
            summary_text,
            embedding_model_version: DEFAULT_EMBEDDING_MODEL.to_string(),
        });
    }
    out
}

/// Normalized parent folder for a chunk's file_path. Returns None when the
/// chunk lives at the repo root (no parent segment).
fn chunk_parent_folder(file_path: &str) -> Option<&str> {
    file_path.rfind('/').map(|idx| &file_path[..idx])
}

/// Project-name resolution for a folder. Top-level sibling dirs under the
/// portfolio root become project names (matches the CodeChunk convention);
/// the root folder itself gets the portfolio dir name as fallback.
fn project_name_for_folder(
    folder_path: &str,
    repo_root: &Path,
    cli_override: Option<&str>,
) -> String {
    if let Some(name) = cli_override {
        return name.to_string();
    }
    if folder_path.is_empty() {
        return repo_root
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();
    }
    folder_path
        .split('/')
        .next()
        .unwrap_or("unknown")
        .to_string()
}

fn is_type_node(node_type: &str) -> bool {
    matches!(
        node_type,
        "struct_item"
            | "enum_item"
            | "trait_item"
            | "class_definition"    // Python
            | "class_declaration"   // TypeScript
            | "interface_declaration"
            | "type_alias_declaration"
            | "enum_declaration"
    )
}

fn is_function_node(node_type: &str) -> bool {
    matches!(
        node_type,
        "function_item"           // Rust
            | "function_definition"   // Python
            | "function_declaration"  // TypeScript
            | "method_definition"
            | "lexical_declaration" // TS arrow fns captured as lexical_declaration
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use code_rag_types::CodeChunk;
    use std::fs;
    use tempfile::TempDir;

    fn mk_code_chunk(
        file_path: &str,
        language: &str,
        identifier: &str,
        node_type: &str,
        signature: Option<&str>,
    ) -> CodeChunk {
        CodeChunk {
            file_path: file_path.to_string(),
            language: language.to_string(),
            identifier: identifier.to_string(),
            node_type: node_type.to_string(),
            code_content: "...".to_string(),
            start_line: 1,
            project_name: "test".to_string(),
            docstring: None,
            signature: signature.map(|s| s.to_string()),
            chunk_id: format!("{file_path}:{identifier}"),
            content_hash: "h".to_string(),
            embedding_model_version: "test".to_string(),
        }
    }

    #[test]
    fn chunk_parent_folder_splits_at_last_slash() {
        assert_eq!(
            chunk_parent_folder("proj/crates/engine/src/lib.rs"),
            Some("proj/crates/engine/src")
        );
        assert_eq!(chunk_parent_folder("toplevel.rs"), None);
    }

    #[test]
    fn build_excludes_vendor_dirs() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        fs::create_dir_all(base.join("src")).unwrap();
        fs::create_dir_all(base.join("target/debug")).unwrap();
        fs::create_dir_all(base.join("node_modules/foo")).unwrap();
        fs::write(base.join("src/lib.rs"), "pub fn a() {}").unwrap();
        fs::write(base.join("target/debug/build.log"), "junk").unwrap();
        fs::write(base.join("node_modules/foo/index.js"), "module").unwrap();

        let chunks = build_folder_chunks(base.to_str().unwrap(), &[], Some("t"));
        // target/ and node_modules/ must never appear as folder chunks.
        for c in &chunks {
            assert!(
                !c.folder_path.contains("target"),
                "found: {}",
                c.folder_path
            );
            assert!(
                !c.folder_path.contains("node_modules"),
                "found: {}",
                c.folder_path
            );
        }
    }

    #[test]
    fn build_produces_module_synonym_in_summary() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        fs::create_dir_all(base.join("crates/engine/src")).unwrap();
        fs::write(
            base.join("crates/engine/src/lib.rs"),
            "pub fn retrieve() {}",
        )
        .unwrap();

        let code_chunks = vec![mk_code_chunk(
            "crates/engine/src/lib.rs",
            "rust",
            "retrieve",
            "function_item",
            Some("pub fn retrieve() -> ()"),
        )];
        let chunks = build_folder_chunks(base.to_str().unwrap(), &code_chunks, Some("t"));
        let src_folder = chunks
            .iter()
            .find(|c| c.folder_path == "crates/engine/src")
            .expect("src folder summarized");
        assert!(
            src_folder.summary_text.contains("(module: src)"),
            "summary: {}",
            src_folder.summary_text
        );
        assert!(
            src_folder.key_functions.contains(&"retrieve".to_string()),
            "keys: {:?}",
            src_folder.key_functions
        );
    }

    #[test]
    fn build_skips_empty_folders() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        fs::create_dir_all(base.join("empty_dir")).unwrap();
        // Only the empty dir exists; with no files/subfolders/keys it must be skipped.
        let chunks = build_folder_chunks(base.to_str().unwrap(), &[], Some("t"));
        assert!(
            !chunks.iter().any(|c| c.folder_path == "empty_dir"),
            "chunks: {chunks:?}"
        );
    }

    #[test]
    fn build_sorts_key_functions_alphabetically() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        fs::create_dir_all(base.join("src")).unwrap();
        fs::write(base.join("src/lib.rs"), "pub fn a() {}").unwrap();

        let code_chunks = vec![
            mk_code_chunk(
                "src/lib.rs",
                "rust",
                "zeta",
                "function_item",
                Some("pub fn zeta()"),
            ),
            mk_code_chunk(
                "src/lib.rs",
                "rust",
                "alpha",
                "function_item",
                Some("pub fn alpha()"),
            ),
            mk_code_chunk(
                "src/lib.rs",
                "rust",
                "alpha",
                "function_item",
                Some("pub fn alpha()"),
            ),
        ];
        let chunks = build_folder_chunks(base.to_str().unwrap(), &code_chunks, Some("t"));
        let src = chunks.iter().find(|c| c.folder_path == "src").unwrap();
        assert_eq!(
            src.key_functions,
            vec!["alpha".to_string(), "zeta".to_string()]
        );
    }

    #[test]
    fn build_caps_key_types_at_max() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        fs::create_dir_all(base.join("src")).unwrap();
        fs::write(base.join("src/lib.rs"), "pub struct S0;").unwrap();

        let code_chunks: Vec<CodeChunk> = (0..30)
            .map(|i| {
                mk_code_chunk(
                    "src/lib.rs",
                    "rust",
                    &format!("Struct{i:02}"),
                    "struct_item",
                    Some(&format!("pub struct Struct{i:02}")),
                )
            })
            .collect();
        let chunks = build_folder_chunks(base.to_str().unwrap(), &code_chunks, Some("t"));
        let src = chunks.iter().find(|c| c.folder_path == "src").unwrap();
        assert_eq!(src.key_types.len(), MAX_KEYS);
    }

    #[test]
    fn build_skips_private_rust_items() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        fs::create_dir_all(base.join("src")).unwrap();
        fs::write(base.join("src/lib.rs"), "fn helper() {}").unwrap();

        let code_chunks = vec![mk_code_chunk(
            "src/lib.rs",
            "rust",
            "helper",
            "function_item",
            Some("fn helper()"),
        )];
        let chunks = build_folder_chunks(base.to_str().unwrap(), &code_chunks, Some("t"));
        let src = chunks.iter().find(|c| c.folder_path == "src").unwrap();
        // `fn helper()` has no `pub ` prefix — excluded.
        assert!(src.key_functions.is_empty());
    }

    #[test]
    fn build_deterministic_across_calls() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        fs::create_dir_all(base.join("src/inner")).unwrap();
        fs::write(base.join("src/lib.rs"), "pub fn a() {}").unwrap();
        fs::write(base.join("src/inner/util.rs"), "pub fn b() {}").unwrap();

        let out1 = build_folder_chunks(base.to_str().unwrap(), &[], Some("t"));
        let out2 = build_folder_chunks(base.to_str().unwrap(), &[], Some("t"));
        let ids1: Vec<&str> = out1.iter().map(|c| c.chunk_id.as_str()).collect();
        let ids2: Vec<&str> = out2.iter().map(|c| c.chunk_id.as_str()).collect();
        assert_eq!(ids1, ids2);
    }

    #[test]
    fn build_registers_subfolders_from_grandparent() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();
        fs::create_dir_all(base.join("crates/engine/src")).unwrap();
        fs::write(base.join("crates/engine/src/lib.rs"), "pub fn a() {}").unwrap();
        let chunks = build_folder_chunks(base.to_str().unwrap(), &[], Some("t"));
        let engine = chunks
            .iter()
            .find(|c| c.folder_path == "crates/engine")
            .expect("crates/engine summarized");
        assert!(
            engine.subfolders.contains(&"src".to_string()),
            "subfolders: {:?}",
            engine.subfolders
        );
    }
}
