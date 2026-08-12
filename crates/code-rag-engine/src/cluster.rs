//! R3: emergent-cluster (Code Raptor) summary rendering — template-based, no
//! LLM. Pure, WASM-safe single source of truth so server-rendered bytes and
//! browser-side reading of `summary_text` never diverge. Mirrors `folder.rs`.

use super::folder::{MAX_KEYS, csv_or};

/// Metadata feeding the cluster summary template. Borrowed view to avoid copies.
pub struct ClusterMeta<'a> {
    pub cluster_id: u32,
    /// Member code-chunk count (functions + types).
    pub member_count: usize,
    /// Distinct file count the members span.
    pub file_count: usize,
    pub key_types: &'a [String],
    pub key_functions: &'a [String],
    /// File basenames the members span (for the "Files:" line).
    pub files: &'a [String],
    /// Most common intra-community relation tag, or "" if unknown.
    pub dominant_relation: &'a str,
    /// R2 cohesion in `[0, 1]`.
    pub cohesion: f32,
    /// Identifier of the most-central (highest-degree) member, or "".
    pub central_member: &'a str,
}

/// Cap on the number of files/keys rendered into the summary (bounds length
/// well under the BGE-small 512-token budget). Reuses folder's `MAX_KEYS`.
pub const MAX_FILES: usize = MAX_KEYS;

/// Render the canonical cluster summary. The first line embeds the
/// `module/subsystem` synonyms so BM25/rerank catch architecture queries
/// ("what are the main subsystems?", "what handles X?") without query-time
/// expansion. Deterministic, pure format-only — re-callable yields identical bytes.
pub fn render_summary(m: &ClusterMeta<'_>) -> String {
    format!(
        "Cluster {}: a module/subsystem spanning {} files ({} definitions).\nKey types: {}\nKey functions: {}\nFiles: {}\nDominant relation: {}\nCohesion: {:.2}\nLikely concern: {}",
        m.cluster_id,
        m.file_count,
        m.member_count,
        csv_or(m.key_types, "none"),
        csv_or(m.key_functions, "none"),
        csv_or(m.files, "none"),
        or_na(m.dominant_relation),
        m.cohesion,
        or_na(m.central_member),
    )
}

fn or_na(s: &str) -> &str {
    if s.is_empty() { "n/a" } else { s }
}

/// Canonicalized metadata tuple → stable hash input for `content_hash`.
/// Depends only on the fields that appear in `summary_text`.
pub fn canonical_tuple(m: &ClusterMeta<'_>) -> String {
    format!(
        "{}|{}|{}|{}|{}|{}|{}|{:.4}|{}",
        m.cluster_id,
        m.member_count,
        m.file_count,
        m.key_types.join(","),
        m.key_functions.join(","),
        m.files.join(","),
        m.dominant_relation,
        m.cohesion,
        m.central_member,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta<'a>(files: &'a [String], dom: &'a str, central: &'a str) -> ClusterMeta<'a> {
        ClusterMeta {
            cluster_id: 0,
            member_count: 3,
            file_count: files.len(),
            key_types: &[],
            key_functions: &[],
            files,
            dominant_relation: dom,
            cohesion: 0.5,
            central_member: central,
        }
    }

    #[test]
    fn render_has_subsystem_synonym() {
        let s = render_summary(&meta(&[], "calls", "retrieve"));
        assert!(s.starts_with("Cluster 0: a module/subsystem spanning 0 files (3 definitions)."));
    }

    #[test]
    fn render_uses_fallbacks() {
        let s = render_summary(&meta(&[], "", ""));
        assert!(s.contains("Key types: none"));
        assert!(s.contains("Files: none"));
        assert!(s.contains("Dominant relation: n/a"));
        assert!(s.contains("Likely concern: n/a"));
    }

    #[test]
    fn render_cohesion_two_decimals() {
        let s = render_summary(&meta(&[], "calls", "x"));
        assert!(s.contains("Cohesion: 0.50"), "got: {s}");
    }

    #[test]
    fn canonical_tuple_changes_with_fields() {
        let a = meta(&[], "calls", "x");
        let mut b = meta(&[], "imports", "x");
        b.cohesion = 0.5;
        assert_ne!(canonical_tuple(&a), canonical_tuple(&b));
    }

    #[test]
    fn render_deterministic() {
        let files = vec!["a.rs".to_string(), "b.rs".to_string()];
        let m = meta(&files, "calls", "foo");
        assert_eq!(render_summary(&m), render_summary(&m));
    }
}
