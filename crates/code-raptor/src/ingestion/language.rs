use tree_sitter::{Language, Node};

/// Trait for language-specific parsing behavior.
///
/// Implement this trait to add support for a new programming language.
/// Each implementation handles grammar loading and query patterns for its language.
/// Docstring extraction (V1.5) overrides the default `None` return per handler.
pub trait LanguageHandler: Send + Sync {
    /// Language identifier (e.g., "rust", "python")
    fn name(&self) -> &'static str;

    /// File extensions this handler supports (e.g., &["rs"] for Rust)
    fn extensions(&self) -> &'static [&'static str];

    /// Get the tree-sitter grammar for this language
    fn grammar(&self) -> Language;

    /// Tree-sitter S-expression query for extracting code elements.
    ///
    /// Must capture:
    /// - `@name` - the identifier of the element
    /// - `@body` - the full element node
    fn query_string(&self) -> &'static str;

    /// Extract docstring from a code element.
    ///
    /// Default returns None. Per-language implementations added in V1.5.
    fn extract_docstring(
        &self,
        _source: &str,
        _node: &Node,
        _source_bytes: &[u8],
    ) -> Option<String> {
        None
    }

    /// Extract function/method call identifiers from a code element's body.
    ///
    /// Walks the AST subtree of the body node to find call expressions.
    /// Returns deduplicated, sorted identifiers. Default returns empty vec.
    fn extract_calls(&self, _source: &str, _node: &Node, _source_bytes: &[u8]) -> Vec<String> {
        Vec::new()
    }
}
