//! A4: File-level summary rendering (template-based, no LLM).
//! Mirror of A2's `folder` module, substituting folder-of-files for single-file.
//! Pure, WASM-safe — single source of truth so server-rendered bytes and
//! browser-side reading of `summary_text` never diverge.

use crate::folder::{basename_of, csv_or};

/// Metadata feeding the file summary template. Borrowed view to avoid copies
/// during ingestion.
pub struct FileMeta<'a> {
    pub file_path: &'a str,
    pub language: &'a str,
    /// Public exports (types + functions) defined in this file.
    pub exports: &'a [String],
    /// External / cross-file imports, pre-formatted as
    /// `"{imported_name} from {source_path}"`.
    pub imports: &'a [String],
    /// Inferred one-sentence purpose. None → fallback rendered.
    pub purpose: Option<&'a str>,
}

/// Cap on exports / imports list length. Keeps `summary_text` under the
/// BGE-small 512-token budget even for re-export-heavy lib.rs files.
/// Matches A2's MAX_KEYS philosophy; raised from the 12 the A4 draft
/// proposed to 16 for symmetry between exports and imports.
pub const MAX_FILE_LIST: usize = 16;

/// Max length (chars) of the `purpose` line. Long docstrings get trimmed.
pub const MAX_PURPOSE_CHARS: usize = 140;

/// Render the canonical file summary. Four lines. The first line embeds a
/// `module:` synonym (the file basename without extension) so BM25/rerank
/// catch queries phrased as "what does the retriever module do?" — users
/// often say "module" for a file, especially in Rust/Python where the file
/// backs a module directly.
///
/// Deterministic. Pure format-only.
pub fn render_summary(m: &FileMeta<'_>) -> String {
    let module = module_name_of(m.file_path);
    format!(
        "File: {} (module: {}, {})\nExports: {}\nImports: {}\nPurpose: {}",
        m.file_path,
        module,
        m.language,
        csv_or(m.exports, "none"),
        csv_or(m.imports, "none"),
        m.purpose.unwrap_or("(none extracted)"),
    )
}

/// Module name for the dual-label: basename with the last extension stripped.
/// `"src/retriever.rs"` → `"retriever"`. `"app.module.ts"` → `"app.module"`.
pub fn module_name_of(file_path: &str) -> &str {
    let base = basename_of(file_path);
    match base.rfind('.') {
        Some(idx) if idx > 0 => &base[..idx],
        _ => base,
    }
}

/// Canonicalized metadata tuple → stable hash input for `content_hash`.
/// Only depends on fields that appear in `summary_text`, so re-ingests
/// with unchanged file metadata produce identical hashes.
pub fn canonical_tuple(m: &FileMeta<'_>) -> String {
    format!(
        "{}|{}|{}|{}|{}",
        m.file_path,
        m.language,
        m.exports.join(","),
        m.imports.join(","),
        m.purpose.unwrap_or(""),
    )
}

/// Trim a candidate purpose string: first line only, collapse whitespace,
/// cap at `MAX_PURPOSE_CHARS`. Returns None for empty input (so callers
/// can fall through to the next source in priority order).
pub fn clean_purpose(raw: &str) -> Option<String> {
    let first_line = raw.lines().next().unwrap_or("").trim();
    if first_line.is_empty() {
        return None;
    }
    let collapsed: String = first_line.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= MAX_PURPOSE_CHARS {
        Some(collapsed)
    } else {
        Some(collapsed.chars().take(MAX_PURPOSE_CHARS).collect())
    }
}

/// Filename-derived purpose fallback: "This file defines {module_name}."
pub fn filename_purpose(file_path: &str) -> String {
    format!("This file defines {}.", module_name_of(file_path))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta<'a>(file_path: &'a str, language: &'a str) -> FileMeta<'a> {
        FileMeta {
            file_path,
            language,
            exports: &[],
            imports: &[],
            purpose: None,
        }
    }

    #[test]
    fn render_summary_has_module_synonym_and_language() {
        let s = render_summary(&meta(
            "code-rag/crates/code-rag-engine/src/retriever.rs",
            "rust",
        ));
        assert!(
            s.starts_with(
                "File: code-rag/crates/code-rag-engine/src/retriever.rs (module: retriever, rust)"
            ),
            "got: {s}"
        );
    }

    #[test]
    fn render_summary_four_lines() {
        let s = render_summary(&meta("a/b.rs", "rust"));
        assert_eq!(s.lines().count(), 4);
    }

    #[test]
    fn render_summary_empty_fields_use_fallbacks() {
        let s = render_summary(&meta("a/b.rs", "rust"));
        assert!(s.contains("Exports: none"));
        assert!(s.contains("Imports: none"));
        assert!(s.contains("Purpose: (none extracted)"));
    }

    #[test]
    fn render_summary_csv_joining() {
        let exports = vec!["Retriever".to_string(), "retrieve".to_string()];
        let imports = vec!["CodeChunk from code_rag_types".to_string()];
        let m = FileMeta {
            file_path: "a/retriever.rs",
            language: "rust",
            exports: &exports,
            imports: &imports,
            purpose: Some("Orchestrates per-intent retrieval."),
        };
        let s = render_summary(&m);
        assert!(s.contains("Exports: Retriever, retrieve"));
        assert!(s.contains("Imports: CodeChunk from code_rag_types"));
        assert!(s.contains("Purpose: Orchestrates per-intent retrieval."));
    }

    #[test]
    fn module_name_strips_last_extension_only() {
        assert_eq!(module_name_of("retriever.rs"), "retriever");
        assert_eq!(module_name_of("app.module.ts"), "app.module");
        assert_eq!(module_name_of("Dockerfile"), "Dockerfile");
        assert_eq!(module_name_of("src/.hidden"), ".hidden");
        assert_eq!(module_name_of(""), "");
    }

    #[test]
    fn render_summary_deterministic_across_calls() {
        let exports = vec!["B".to_string(), "A".to_string()];
        let m = FileMeta {
            file_path: "x.rs",
            language: "rust",
            exports: &exports,
            imports: &[],
            purpose: None,
        };
        assert_eq!(render_summary(&m), render_summary(&m));
    }

    #[test]
    fn render_summary_bounded_for_max_list() {
        let exports: Vec<String> = (0..MAX_FILE_LIST).map(|i| format!("Name{i:02}")).collect();
        let imports: Vec<String> = (0..MAX_FILE_LIST)
            .map(|i| format!("Imp{i:02} from mod::path"))
            .collect();
        let m = FileMeta {
            file_path: "crates/foo/src/lib.rs",
            language: "rust",
            exports: &exports,
            imports: &imports,
            purpose: Some("Central facade."),
        };
        let s = render_summary(&m);
        assert!(s.len() < 800, "summary too long: {} bytes", s.len());
    }

    #[test]
    fn clean_purpose_trims_and_caps() {
        assert_eq!(
            clean_purpose("  Orchestrates retrieval.\nSecond line. "),
            Some("Orchestrates retrieval.".to_string())
        );
        assert_eq!(clean_purpose(""), None);
        assert_eq!(clean_purpose("   \n\n"), None);
        let long = "a".repeat(200);
        let cleaned = clean_purpose(&long).unwrap();
        assert_eq!(cleaned.chars().count(), MAX_PURPOSE_CHARS);
    }

    #[test]
    fn filename_purpose_uses_module_name() {
        assert_eq!(
            filename_purpose("src/retriever.rs"),
            "This file defines retriever."
        );
    }

    #[test]
    fn canonical_tuple_changes_with_fields() {
        let empty: Vec<String> = Vec::new();
        let a = FileMeta {
            file_path: "x.rs",
            language: "rust",
            exports: &empty,
            imports: &empty,
            purpose: None,
        };
        let b = FileMeta {
            file_path: "x.rs",
            language: "rust",
            exports: &empty,
            imports: &empty,
            purpose: Some("changed"),
        };
        assert_ne!(canonical_tuple(&a), canonical_tuple(&b));
    }
}
