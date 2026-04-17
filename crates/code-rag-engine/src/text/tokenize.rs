/// Lowercase, split on non-alphanumeric, drop empty segments.
///
/// Canonical tokenizer for BM25 indexing and search. Must match bit-for-bit
/// on server and browser — otherwise IDF tables built at ingestion time
/// produce different scores than queries issued in the browser.
pub fn tokenize(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}
