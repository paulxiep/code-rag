//! A4: Build FileChunks for a repository.
//!
//! One FileChunk per source file that produced CodeChunks. Summary fields:
//! - `exports` ← public types/functions from CodeChunks (same visibility
//!   heuristic A2 uses for folder `key_types`/`key_functions`).
//! - `imports` ← `LanguageHandler::extract_file_imports` output (C1-era API),
//!   formatted as `"{name} from {source_path}"`.
//! - `purpose` ← first line of module doc, else first docstring in file,
//!   else filename-derived fallback.
//!
//! Template rendering lives in `code_rag_engine::file` so server and WASM
//! see identical `summary_text` bytes.

use std::collections::{BTreeMap, BTreeSet};

use code_rag_engine::file::{
    self as fsum, FileMeta, MAX_FILE_LIST, clean_purpose, filename_purpose,
};
use code_rag_engine::folder as folder_helpers;
use code_rag_types::{CodeChunk, FileChunk, ModuleDocChunk, content_hash, deterministic_chunk_id};

use super::language::ImportInfo;
use super::{DEFAULT_EMBEDDING_MODEL, ImportsMap};

/// Build FileChunks for every source file that produced CodeChunks in this
/// project. Files with zero CodeChunks (parser rejected, unsupported lang)
/// are skipped — the FileChunk would carry no useful signal.
///
/// `imports_map` is the `file_path → Vec<ImportInfo>` produced by the same
/// `run_ingestion` that assembled `code_chunks`. Files not present in the
/// map get an empty imports list (renders as `Imports: none`).
///
/// `module_doc_chunks` is consulted first for the `purpose` field; then the
/// file's own first docstring (smallest `start_line`); then a filename-derived
/// fallback.
pub fn build_file_chunks(
    code_chunks: &[CodeChunk],
    module_doc_chunks: &[ModuleDocChunk],
    imports_map: &ImportsMap,
    project_name_override: Option<&str>,
) -> Vec<FileChunk> {
    // Group CodeChunks by file_path, preserving insertion order per file so
    // `start_line` comparisons work for the docstring fallback.
    let mut by_file: BTreeMap<String, Vec<&CodeChunk>> = BTreeMap::new();
    for c in code_chunks {
        by_file.entry(c.file_path.clone()).or_default().push(c);
    }

    // Fast lookup for module docs by file path.
    let module_doc_by_file: BTreeMap<&str, &ModuleDocChunk> = module_doc_chunks
        .iter()
        .map(|m| (m.file_path.as_str(), m))
        .collect();

    let mut out = Vec::with_capacity(by_file.len());
    for (file_path, file_chunks) in by_file {
        if file_chunks.is_empty() {
            continue;
        }

        // Language: all chunks in one file share a language (parser-driven).
        let language = file_chunks[0].language.clone();

        // Project name: CodeChunks already carry the resolved project_name.
        // If a CLI override is active, prefer it for consistency with A2's
        // `project_name_for_folder`; otherwise use the chunk's value.
        let project_name = project_name_override
            .map(str::to_string)
            .unwrap_or_else(|| file_chunks[0].project_name.clone());

        // --- exports: public type+function identifiers, sorted, deduped, capped.
        let mut exports_set: BTreeSet<String> = BTreeSet::new();
        for c in &file_chunks {
            if !folder_helpers::is_public(&c.language, c.signature.as_deref(), &c.identifier) {
                continue;
            }
            if is_type_node(&c.node_type) || is_function_node(&c.node_type) {
                exports_set.insert(c.identifier.clone());
            }
        }
        let exports: Vec<String> = exports_set.into_iter().take(MAX_FILE_LIST).collect();

        // --- imports: from the C1 imports_map.
        let imports = format_imports(imports_map.get(&file_path));

        // --- purpose: module_doc → first docstring → filename fallback.
        let purpose = purpose_for_file(
            &file_path,
            module_doc_by_file.get(file_path.as_str()).copied(),
            &file_chunks,
        );

        let meta = FileMeta {
            file_path: &file_path,
            language: &language,
            exports: &exports,
            imports: &imports,
            purpose: purpose.as_deref(),
        };
        let summary_text = fsum::render_summary(&meta);
        let canonical = fsum::canonical_tuple(&meta);

        out.push(FileChunk {
            chunk_id: deterministic_chunk_id(&format!("file:{}", file_path), &summary_text),
            content_hash: content_hash(&canonical),
            file_path,
            project_name,
            language,
            exports,
            imports,
            purpose,
            summary_text,
            embedding_model_version: DEFAULT_EMBEDDING_MODEL.to_string(),
        });
    }
    out
}

/// Format a slice of ImportInfo into display strings, dedup + sort, cap at MAX.
fn format_imports(raw: Option<&Vec<ImportInfo>>) -> Vec<String> {
    let Some(raw) = raw else {
        return Vec::new();
    };
    let mut set: BTreeSet<String> = BTreeSet::new();
    for imp in raw {
        set.insert(format!("{} from {}", imp.imported_name, imp.source_path));
    }
    set.into_iter().take(MAX_FILE_LIST).collect()
}

/// Resolve `purpose` in priority order — module doc, first docstring, filename.
fn purpose_for_file(
    file_path: &str,
    module_doc: Option<&ModuleDocChunk>,
    chunks: &[&CodeChunk],
) -> Option<String> {
    // 1. Module doc for this file path.
    if let Some(md) = module_doc
        && let Some(cleaned) = clean_purpose(&md.doc_content)
    {
        return Some(cleaned);
    }

    // 2. First docstring in file (smallest start_line with a non-empty doc).
    let first_doc = chunks
        .iter()
        .filter_map(|c| c.docstring.as_deref().map(|d| (c.start_line, d)))
        .min_by_key(|(line, _)| *line);
    if let Some((_, doc)) = first_doc
        && let Some(cleaned) = clean_purpose(doc)
    {
        return Some(cleaned);
    }

    // 3. Filename-derived fallback.
    Some(filename_purpose(file_path))
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn mk_code_chunk(
        file_path: &str,
        language: &str,
        identifier: &str,
        node_type: &str,
        signature: Option<&str>,
        start_line: usize,
        docstring: Option<&str>,
    ) -> CodeChunk {
        CodeChunk {
            file_path: file_path.to_string(),
            language: language.to_string(),
            identifier: identifier.to_string(),
            node_type: node_type.to_string(),
            code_content: "...".to_string(),
            start_line,
            project_name: "proj".to_string(),
            docstring: docstring.map(|s| s.to_string()),
            signature: signature.map(|s| s.to_string()),
            chunk_id: format!("{file_path}:{identifier}"),
            content_hash: "h".to_string(),
            embedding_model_version: "test".to_string(),
        }
    }

    fn mk_module_doc(file_path: &str, doc_content: &str) -> ModuleDocChunk {
        ModuleDocChunk {
            file_path: file_path.to_string(),
            module_name: "m".to_string(),
            doc_content: doc_content.to_string(),
            project_name: "proj".to_string(),
            chunk_id: "id".to_string(),
            content_hash: "h".to_string(),
            embedding_model_version: "test".to_string(),
        }
    }

    fn mk_import(name: &str, source: &str) -> ImportInfo {
        ImportInfo {
            imported_name: name.to_string(),
            source_path: source.to_string(),
        }
    }

    #[test]
    fn build_emits_one_chunk_per_file_with_code_chunks() {
        let chunks = vec![
            mk_code_chunk(
                "a.rs",
                "rust",
                "foo",
                "function_item",
                Some("pub fn foo()"),
                1,
                None,
            ),
            mk_code_chunk(
                "b.rs",
                "rust",
                "bar",
                "function_item",
                Some("pub fn bar()"),
                1,
                None,
            ),
        ];
        let out = build_file_chunks(&chunks, &[], &HashMap::new(), Some("proj"));
        assert_eq!(out.len(), 2);
        let paths: Vec<&str> = out.iter().map(|c| c.file_path.as_str()).collect();
        assert!(paths.contains(&"a.rs"));
        assert!(paths.contains(&"b.rs"));
    }

    #[test]
    fn build_collects_public_exports_only() {
        let chunks = vec![
            mk_code_chunk(
                "lib.rs",
                "rust",
                "PubStruct",
                "struct_item",
                Some("pub struct PubStruct"),
                1,
                None,
            ),
            mk_code_chunk(
                "lib.rs",
                "rust",
                "priv_fn",
                "function_item",
                Some("fn priv_fn()"),
                5,
                None,
            ),
            mk_code_chunk(
                "lib.rs",
                "rust",
                "pub_fn",
                "function_item",
                Some("pub fn pub_fn()"),
                10,
                None,
            ),
        ];
        let out = build_file_chunks(&chunks, &[], &HashMap::new(), Some("proj"));
        let fc = &out[0];
        assert_eq!(
            fc.exports,
            vec!["PubStruct".to_string(), "pub_fn".to_string()]
        );
    }

    #[test]
    fn build_caps_exports_at_16() {
        let chunks: Vec<CodeChunk> = (0..30)
            .map(|i| {
                mk_code_chunk(
                    "lib.rs",
                    "rust",
                    &format!("PubFn{i:02}"),
                    "function_item",
                    Some(&format!("pub fn PubFn{i:02}()")),
                    i,
                    None,
                )
            })
            .collect();
        let out = build_file_chunks(&chunks, &[], &HashMap::new(), Some("proj"));
        assert_eq!(out[0].exports.len(), MAX_FILE_LIST);
        assert_eq!(MAX_FILE_LIST, 16);
    }

    #[test]
    fn build_formats_imports_from_importinfo() {
        let chunks = vec![mk_code_chunk(
            "lib.rs",
            "rust",
            "foo",
            "function_item",
            Some("pub fn foo()"),
            1,
            None,
        )];
        let mut imap = HashMap::new();
        imap.insert(
            "lib.rs".to_string(),
            vec![
                mk_import("CodeChunk", "code_rag_types"),
                mk_import("normalize_path", "crate::ingestion::mod"),
            ],
        );
        let out = build_file_chunks(&chunks, &[], &imap, Some("proj"));
        // Sorted alphabetically.
        assert_eq!(
            out[0].imports,
            vec![
                "CodeChunk from code_rag_types".to_string(),
                "normalize_path from crate::ingestion::mod".to_string(),
            ]
        );
    }

    #[test]
    fn build_dedups_imports() {
        let chunks = vec![mk_code_chunk(
            "lib.rs",
            "rust",
            "foo",
            "function_item",
            Some("pub fn foo()"),
            1,
            None,
        )];
        let mut imap = HashMap::new();
        imap.insert(
            "lib.rs".to_string(),
            vec![
                mk_import("X", "a::b"),
                mk_import("X", "a::b"), // duplicate
                mk_import("Y", "a::b"),
            ],
        );
        let out = build_file_chunks(&chunks, &[], &imap, Some("proj"));
        assert_eq!(
            out[0].imports,
            vec!["X from a::b".to_string(), "Y from a::b".to_string()]
        );
    }

    #[test]
    fn build_purpose_prefers_module_doc_over_docstring() {
        let chunks = vec![mk_code_chunk(
            "lib.rs",
            "rust",
            "foo",
            "function_item",
            Some("pub fn foo()"),
            1,
            Some("Function-level doc"),
        )];
        let mods = vec![mk_module_doc("lib.rs", "Module-level doc.\n\nMore detail.")];
        let out = build_file_chunks(&chunks, &mods, &HashMap::new(), Some("proj"));
        assert_eq!(out[0].purpose.as_deref(), Some("Module-level doc."));
    }

    #[test]
    fn build_purpose_falls_back_to_first_docstring_by_start_line() {
        let chunks = vec![
            mk_code_chunk(
                "lib.rs",
                "rust",
                "bar",
                "function_item",
                Some("pub fn bar()"),
                50,
                Some("Doc for bar"),
            ),
            mk_code_chunk(
                "lib.rs",
                "rust",
                "foo",
                "function_item",
                Some("pub fn foo()"),
                10,
                Some("Doc for foo"),
            ),
        ];
        let out = build_file_chunks(&chunks, &[], &HashMap::new(), Some("proj"));
        // Smallest start_line wins regardless of input order.
        assert_eq!(out[0].purpose.as_deref(), Some("Doc for foo"));
    }

    #[test]
    fn build_purpose_filename_fallback() {
        let chunks = vec![mk_code_chunk(
            "src/retriever.rs",
            "rust",
            "foo",
            "function_item",
            Some("pub fn foo()"),
            1,
            None,
        )];
        let out = build_file_chunks(&chunks, &[], &HashMap::new(), Some("proj"));
        assert_eq!(
            out[0].purpose.as_deref(),
            Some("This file defines retriever.")
        );
    }

    #[test]
    fn build_renders_template_with_module_synonym() {
        let chunks = vec![mk_code_chunk(
            "crates/engine/src/retriever.rs",
            "rust",
            "retrieve",
            "function_item",
            Some("pub fn retrieve()"),
            1,
            None,
        )];
        let out = build_file_chunks(&chunks, &[], &HashMap::new(), Some("proj"));
        assert!(out[0].summary_text.contains("(module: retriever, rust)"));
        assert!(out[0].summary_text.contains("Exports: retrieve"));
    }

    #[test]
    fn build_deterministic_across_calls() {
        let chunks = vec![mk_code_chunk(
            "a.rs",
            "rust",
            "foo",
            "function_item",
            Some("pub fn foo()"),
            1,
            None,
        )];
        let out1 = build_file_chunks(&chunks, &[], &HashMap::new(), Some("proj"));
        let out2 = build_file_chunks(&chunks, &[], &HashMap::new(), Some("proj"));
        assert_eq!(out1[0].chunk_id, out2[0].chunk_id);
        assert_eq!(out1[0].summary_text, out2[0].summary_text);
    }

    #[test]
    fn build_chunk_id_distinct_from_folder_id_for_same_path() {
        // If we ever key the folder:src/lib.rs → same chunk_id as file:src/lib.rs,
        // RRF fusion across types would collapse both.
        let chunks = vec![mk_code_chunk(
            "src/lib.rs",
            "rust",
            "foo",
            "function_item",
            Some("pub fn foo()"),
            1,
            None,
        )];
        let out = build_file_chunks(&chunks, &[], &HashMap::new(), Some("proj"));
        let folder_id = deterministic_chunk_id("src/lib.rs", &out[0].summary_text);
        assert_ne!(out[0].chunk_id, folder_id);
    }

    #[test]
    fn build_keeps_files_with_only_private_items() {
        // Exports: none — but purpose + filename fallback still give signal.
        let chunks = vec![mk_code_chunk(
            "internal.rs",
            "rust",
            "helper",
            "function_item",
            Some("fn helper()"),
            1,
            None,
        )];
        let out = build_file_chunks(&chunks, &[], &HashMap::new(), Some("proj"));
        assert_eq!(out.len(), 1);
        assert!(out[0].exports.is_empty());
        assert!(out[0].summary_text.contains("Exports: none"));
    }

    #[test]
    fn build_ignores_files_absent_from_code_chunks() {
        // File with no CodeChunks (e.g. parse failure) gets no FileChunk.
        let out = build_file_chunks(&[], &[], &HashMap::new(), Some("proj"));
        assert!(out.is_empty());
    }

    #[test]
    fn build_windows_path_normalization_not_needed() {
        // Ingestion normalizes paths before producing CodeChunks (see
        // ingestion::mod::normalize_path). build_file_chunks just honors
        // whatever file_path CodeChunks carry — they are already forward-slash.
        // This smoke test asserts that assumption stays honest.
        let chunks = vec![mk_code_chunk(
            "crates/engine/src/lib.rs",
            "rust",
            "foo",
            "function_item",
            Some("pub fn foo()"),
            1,
            None,
        )];
        let out = build_file_chunks(&chunks, &[], &HashMap::new(), Some("proj"));
        assert_eq!(out[0].file_path, "crates/engine/src/lib.rs");
    }
}
