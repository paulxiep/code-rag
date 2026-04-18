/// Build searchable_text from high-signal fields only.
/// Excludes code body and calls — these dilute BM25 signal.
///
/// Two BM25 optimizations:
/// 1. Identifier repeated 2x — simulates field-level boosting (LanceDB single-column FTS)
/// 2. camelCase/PascalCase split into component words alongside original
pub fn build_searchable_text(
    identifier: &str,
    signature: Option<&str>,
    docstring: Option<&str>,
) -> String {
    let mut parts = Vec::new();

    let split = split_camel_case(identifier);
    if split != identifier.to_lowercase() {
        parts.push(format!("{} {} {}", identifier, identifier, split));
    } else {
        parts.push(format!("{} {}", identifier, identifier));
    }

    if let Some(sig) = signature {
        parts.push(sig.to_string());
    }
    if let Some(doc) = docstring
        && !doc.is_empty()
    {
        parts.push(doc.to_string());
    }
    parts.join("\n")
}

/// Split camelCase/PascalCase into lowercase words.
/// "VectorStore" → "vector store"
/// "parseHTTPResponse" → "parse http response"
/// "snake_case" → "snake_case" (unchanged, already split by tokenizer)
pub fn split_camel_case(s: &str) -> String {
    let mut words = Vec::new();
    let mut current = String::new();
    let chars: Vec<char> = s.chars().collect();

    for i in 0..chars.len() {
        let c = chars[i];
        if c.is_uppercase() && !current.is_empty() {
            let prev_upper = i > 0 && chars[i - 1].is_uppercase();
            let next_lower = i + 1 < chars.len() && chars[i + 1].is_lowercase();
            if !prev_upper || next_lower {
                words.push(current.to_lowercase());
                current = String::new();
            }
        }
        current.push(c);
    }
    if !current.is_empty() {
        words.push(current.to_lowercase());
    }
    words.join(" ")
}
