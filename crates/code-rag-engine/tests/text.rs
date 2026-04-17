//! Consolidated tests for `code_rag_engine::text`.
//!
//! Supersets the pre-A1 test coverage scattered across
//! `code-rag-ui::text_search::tests` and inline asserts in downstream crates.

use code_rag_engine::text::{
    Bm25Params, IdfTable, build_searchable_text, score, split_camel_case, tokenize,
};
use std::collections::HashMap;

// ---- tokenize ---------------------------------------------------------------

#[test]
fn tokenize_lowercases_and_splits_snake_case() {
    assert_eq!(
        tokenize("distance_to_relevance"),
        vec!["distance", "to", "relevance"]
    );
}

#[test]
fn tokenize_case_insensitive_keeps_joined_camel_case() {
    // Tokenizer splits only on non-alphanumeric, so "VectorStore" is one token.
    // camelCase splitting is a separate concern handled by build_searchable_text.
    assert_eq!(tokenize("VectorStore"), vec!["vectorstore"]);
}

#[test]
fn tokenize_drops_empty_segments() {
    assert_eq!(tokenize("foo   bar"), vec!["foo", "bar"]);
    assert_eq!(tokenize("--foo--"), vec!["foo"]);
    assert!(tokenize("").is_empty());
}

#[test]
fn tokenize_mixed_punctuation() {
    assert_eq!(
        tokenize("foo.bar,baz;qux"),
        vec!["foo", "bar", "baz", "qux"]
    );
}

// ---- IdfTable ---------------------------------------------------------------

fn sample_idf() -> IdfTable {
    let mut doc_frequencies = HashMap::new();
    doc_frequencies.insert("fn".to_string(), 50); // common
    doc_frequencies.insert("retrieve".to_string(), 2); // rare
    doc_frequencies.insert("pub".to_string(), 45); // common
    doc_frequencies.insert("search".to_string(), 5); // moderate
    IdfTable {
        num_docs: 100,
        doc_frequencies,
    }
}

#[test]
fn idf_rare_greater_than_common() {
    let idf = sample_idf();
    assert!(
        idf.idf("retrieve") > idf.idf("fn"),
        "rare term must score higher than common term"
    );
}

#[test]
fn idf_unknown_term_is_positive() {
    let idf = sample_idf();
    assert!(idf.idf("nonexistent") > 0.0);
}

#[test]
fn idf_build_empty_corpus() {
    let idf = IdfTable::build(std::iter::empty::<&str>());
    assert_eq!(idf.num_docs, 0);
    assert!(idf.doc_frequencies.is_empty());
}

#[test]
fn idf_build_counts_unique_tokens_per_doc() {
    // "foo foo bar" contributes +1 for foo and +1 for bar, not +2 for foo.
    let idf = IdfTable::build(["foo foo bar", "foo", "baz"]);
    assert_eq!(idf.num_docs, 3);
    assert_eq!(*idf.doc_frequencies.get("foo").unwrap(), 2);
    assert_eq!(*idf.doc_frequencies.get("bar").unwrap(), 1);
    assert_eq!(*idf.doc_frequencies.get("baz").unwrap(), 1);
}

#[test]
fn idf_roundtrip_serde_preserves_shape() {
    let idf = sample_idf();
    let json = serde_json::to_string(&idf).unwrap();
    let back: IdfTable = serde_json::from_str(&json).unwrap();
    assert_eq!(back.num_docs, idf.num_docs);
    assert_eq!(back.doc_frequencies, idf.doc_frequencies);
}

#[test]
fn idf_deserializes_legacy_two_field_shape() {
    // Pre-A1 `index.json` files have exactly these two fields. A1 must stay
    // byte-compatible.
    let json = r#"{"num_docs":3,"doc_frequencies":{"foo":2,"bar":1}}"#;
    let idf: IdfTable = serde_json::from_str(json).unwrap();
    assert_eq!(idf.num_docs, 3);
    assert_eq!(*idf.doc_frequencies.get("foo").unwrap(), 2);
}

// ---- split_camel_case -------------------------------------------------------

#[test]
fn split_camel_case_pascal() {
    assert_eq!(split_camel_case("VectorStore"), "vector store");
}

#[test]
fn split_camel_case_acronym_run() {
    assert_eq!(split_camel_case("parseHTTPResponse"), "parse http response");
}

#[test]
fn split_camel_case_snake_unchanged() {
    // snake_case has no uppercase letters to split on.
    assert_eq!(split_camel_case("snake_case"), "snake_case");
}

// ---- build_searchable_text --------------------------------------------------

#[test]
fn build_searchable_text_boosts_identifier_twice() {
    let out = build_searchable_text("foo", None, None);
    // "foo foo" appears because identifier is boosted 2x.
    assert!(out.contains("foo foo"));
}

#[test]
fn build_searchable_text_includes_split_for_camel() {
    let out = build_searchable_text("VectorStore", None, None);
    // camelCase split appended alongside original.
    assert!(out.contains("vector store"), "missing split: {out}");
    assert!(out.contains("VectorStore"));
}

#[test]
fn build_searchable_text_omits_empty_docstring() {
    let with_empty = build_searchable_text("foo", None, Some(""));
    let without = build_searchable_text("foo", None, None);
    assert_eq!(with_empty, without);
}

#[test]
fn build_searchable_text_includes_signature_and_docstring() {
    let out = build_searchable_text("foo", Some("fn foo()"), Some("does a thing"));
    assert!(out.contains("fn foo()"));
    assert!(out.contains("does a thing"));
}

// ---- bm25::score ------------------------------------------------------------

#[test]
fn bm25_zero_on_empty_query() {
    let idf = sample_idf();
    let docs = vec!["search".to_string()];
    let empty_query: Vec<String> = vec![];
    assert_eq!(
        score(&empty_query, &docs, 1.0, &idf, Bm25Params::default()),
        0.0
    );
}

#[test]
fn bm25_zero_when_no_terms_match() {
    let idf = sample_idf();
    let query = vec!["unrelated".to_string()];
    let docs = vec!["search".to_string()];
    // "unrelated" has df=0 so its idf is positive but tf=0 in doc, so score = 0.
    assert_eq!(score(&query, &docs, 1.0, &idf, Bm25Params::default()), 0.0);
}

#[test]
fn bm25_rare_term_scores_higher_than_common() {
    let idf = sample_idf();
    let rare = vec!["retrieve".to_string()];
    let common = vec!["fn".to_string()];
    let doc_rare = vec!["retrieve".to_string()];
    let doc_common = vec!["fn".to_string()];
    let p = Bm25Params::default();
    let s_rare = score(&rare, &doc_rare, 1.0, &idf, p);
    let s_common = score(&common, &doc_common, 1.0, &idf, p);
    assert!(s_rare > s_common, "rare={s_rare} common={s_common}");
}

#[test]
fn bm25_monotone_in_tf() {
    let idf = sample_idf();
    let query = vec!["retrieve".to_string()];
    let once = vec!["retrieve".to_string()];
    let twice = vec!["retrieve".to_string(), "retrieve".to_string()];
    let p = Bm25Params::default();
    // Hold avg_doc_len constant so length normalization doesn't flip the
    // inequality — we're isolating the TF saturation curve here.
    assert!(score(&query, &twice, 2.0, &idf, p) > score(&query, &once, 2.0, &idf, p));
}

#[test]
fn bm25_fallback_when_avg_doc_len_zero() {
    // Passing 0.0 triggers the per-doc fallback so downstream code that hasn't
    // precomputed avg_doc_len still produces non-zero scores.
    let idf = sample_idf();
    let query = vec!["retrieve".to_string()];
    let doc = vec!["retrieve".to_string()];
    let p = Bm25Params::default();
    let s = score(&query, &doc, 0.0, &idf, p);
    assert!(s > 0.0);
}
