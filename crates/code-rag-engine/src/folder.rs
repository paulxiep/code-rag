//! A2: Folder-level summary rendering (template-based, no LLM) and
//! public-visibility heuristics. Pure, WASM-safe — single source of truth
//! so server-rendered bytes and browser-side reading of `summary_text`
//! never diverge.

/// Metadata feeding the folder summary template. Borrowed view to avoid
/// copies during ingestion.
pub struct FolderMeta<'a> {
    pub folder_path: &'a str,
    pub file_count: usize,
    pub languages: &'a [String],
    pub key_types: &'a [String],
    pub key_functions: &'a [String],
    pub subfolders: &'a [String],
}

/// Maximum number of key types/functions to include in the summary.
/// Bounds `summary_text` to stay well under the BGE-small 512-token budget.
pub const MAX_KEYS: usize = 12;

/// Render the canonical folder summary. The first line embeds a `module:`
/// synonym (the folder basename) so BM25/rerank catch queries phrased as
/// "what does the X module do?" — users often say "module" for a folder,
/// especially in Rust-heavy contexts where `mod x` backs folder `x/`.
///
/// Deterministic. Pure format-only. Re-callable produces identical bytes.
pub fn render_summary(m: &FolderMeta<'_>) -> String {
    let basename = basename_of(m.folder_path);
    format!(
        "Folder: {} (module: {})\nContains: {} files ({})\nKey types: {}\nKey functions: {}\nSubfolders: {}",
        m.folder_path,
        basename,
        m.file_count,
        csv_or(m.languages, "n/a"),
        csv_or(m.key_types, "none"),
        csv_or(m.key_functions, "none"),
        csv_or(m.subfolders, "none"),
    )
}

pub(crate) fn csv_or(items: &[String], fallback: &str) -> String {
    if items.is_empty() {
        fallback.to_string()
    } else {
        items.join(", ")
    }
}

/// Last path component (treats both '/' and '\\' as separators for safety,
/// though ingestion normalizes to '/'). Empty string if `path` is
/// empty or ends in a separator with no trailing segment.
pub(crate) fn basename_of(path: &str) -> &str {
    path.rsplit(['/', '\\'])
        .find(|s| !s.is_empty())
        .unwrap_or("")
}

/// Canonicalized metadata tuple → stable hash input for `content_hash`.
/// Only depends on the fields that appear in `summary_text`.
pub fn canonical_tuple(m: &FolderMeta<'_>) -> String {
    format!(
        "{}|{}|{}|{}|{}|{}",
        m.folder_path,
        m.file_count,
        m.languages.join(","),
        m.key_types.join(","),
        m.key_functions.join(","),
        m.subfolders.join(",")
    )
}

/// Public-visibility heuristic per language.
///
/// - Rust: signature starts with `pub ` (excludes `pub(crate)` / `pub(super)` —
///   conservative; A2 accepts missing the latter in exchange for no false
///   positives on private items).
/// - Python: identifier does not start with `_` (single underscore convention).
///   Dunder methods (`__init__`) pass through.
/// - TypeScript: signature contains the `export` token.
/// - Other languages: treat as public (permissive fallback).
pub fn is_public(language: &str, signature: Option<&str>, identifier: &str) -> bool {
    match language {
        "rust" => signature.is_some_and(|s| s.trim_start().starts_with("pub ")),
        "python" => !identifier.starts_with('_') || identifier.starts_with("__"),
        "typescript" => signature.is_some_and(has_export_token),
        _ => true,
    }
}

fn has_export_token(signature: &str) -> bool {
    signature
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .any(|tok| tok == "export")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta<'a>(folder_path: &'a str, key_types: &'a [String]) -> FolderMeta<'a> {
        FolderMeta {
            folder_path,
            file_count: 0,
            languages: &[],
            key_types,
            key_functions: &[],
            subfolders: &[],
        }
    }

    #[test]
    fn render_summary_has_module_synonym() {
        let s = render_summary(&meta("code-rag/crates/code-rag-engine/src", &[]));
        assert!(
            s.starts_with("Folder: code-rag/crates/code-rag-engine/src (module: src)"),
            "got: {s}"
        );
    }

    #[test]
    fn render_summary_five_lines() {
        let s = render_summary(&meta("a/b", &[]));
        assert_eq!(s.lines().count(), 5);
    }

    #[test]
    fn render_summary_empty_fields_use_fallbacks() {
        let s = render_summary(&meta("a/b", &[]));
        assert!(s.contains("Contains: 0 files (n/a)"));
        assert!(s.contains("Key types: none"));
        assert!(s.contains("Key functions: none"));
        assert!(s.contains("Subfolders: none"));
    }

    #[test]
    fn render_summary_csv_joining() {
        let langs = vec!["rust".to_string(), "python".to_string()];
        let m = FolderMeta {
            folder_path: "a",
            file_count: 3,
            languages: &langs,
            key_types: &[],
            key_functions: &[],
            subfolders: &[],
        };
        let s = render_summary(&m);
        assert!(s.contains("Contains: 3 files (rust, python)"));
    }

    #[test]
    fn render_summary_deterministic() {
        let types = vec!["Beta".to_string(), "Alpha".to_string()];
        let m = FolderMeta {
            folder_path: "x",
            file_count: 1,
            languages: &[],
            key_types: &types,
            key_functions: &[],
            subfolders: &[],
        };
        assert_eq!(render_summary(&m), render_summary(&m));
    }

    #[test]
    fn render_summary_bounded_for_max_keys() {
        let names: Vec<String> = (0..MAX_KEYS).map(|i| format!("Name{i:02}")).collect();
        let m = FolderMeta {
            folder_path: "crates/foo/src",
            file_count: 12,
            languages: &["rust".to_string()],
            key_types: &names,
            key_functions: &names,
            subfolders: &[],
        };
        let s = render_summary(&m);
        // 12 names × ~8 chars × 2 categories + framing — should stay under 500.
        assert!(s.len() < 500, "summary too long: {} bytes", s.len());
    }

    #[test]
    fn basename_handles_trailing_slash() {
        assert_eq!(basename_of("code-rag/"), "code-rag");
        assert_eq!(basename_of("code-rag"), "code-rag");
        assert_eq!(basename_of(""), "");
    }

    #[test]
    fn canonical_tuple_changes_with_fields() {
        let empty: Vec<String> = Vec::new();
        let a = FolderMeta {
            folder_path: "x",
            file_count: 1,
            languages: &empty,
            key_types: &empty,
            key_functions: &empty,
            subfolders: &empty,
        };
        let b = FolderMeta {
            folder_path: "x",
            file_count: 2,
            languages: &empty,
            key_types: &empty,
            key_functions: &empty,
            subfolders: &empty,
        };
        assert_ne!(canonical_tuple(&a), canonical_tuple(&b));
    }

    #[test]
    fn visibility_rust() {
        assert!(is_public("rust", Some("pub fn foo()"), "foo"));
        assert!(!is_public("rust", Some("pub(crate) fn foo()"), "foo"));
        assert!(!is_public("rust", Some("fn foo()"), "foo"));
        assert!(!is_public("rust", None, "foo"));
    }

    #[test]
    fn visibility_python() {
        assert!(is_public("python", None, "process"));
        assert!(!is_public("python", None, "_helper"));
        // Dunder methods still count as public — they're part of the type API.
        assert!(is_public("python", None, "__init__"));
    }

    #[test]
    fn visibility_typescript() {
        assert!(is_public(
            "typescript",
            Some("export function foo()"),
            "foo"
        ));
        assert!(is_public(
            "typescript",
            Some("export const Button = () =>"),
            "Button"
        ));
        assert!(!is_public("typescript", Some("function foo()"), "foo"));
        assert!(!is_public("typescript", None, "foo"));
        // `exported` looking token must not match `export`.
        assert!(!is_public(
            "typescript",
            Some("const exported_value = 1"),
            "exported_value"
        ));
    }

    #[test]
    fn visibility_unknown_language_permissive() {
        assert!(is_public("ruby", None, "anything"));
    }
}
