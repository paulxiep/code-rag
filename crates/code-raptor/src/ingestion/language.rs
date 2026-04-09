use tree_sitter::{Language, Node};

/// An import found in a source file. Used for tier-2 (import-based) edge resolution.
/// Local to code-raptor; not stored in LanceDB.
#[derive(Debug, Clone)]
pub struct ImportInfo {
    /// The imported symbol name, e.g. "normalize_path"
    pub imported_name: String,
    /// The source module path, e.g. "crate::ingestion::mod" or "./utils"
    pub source_path: String,
}

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

    /// Extract import declarations from the file's root AST node (C1).
    ///
    /// Returns imported symbol names with their source module paths.
    /// Used for tier-2 (import-based) call edge resolution.
    /// Default returns empty vec.
    fn extract_file_imports(
        &self,
        _source: &str,
        _root: &Node,
        _source_bytes: &[u8],
    ) -> Vec<ImportInfo> {
        Vec::new()
    }

    /// Extract declaration signature from a code element (B3).
    ///
    /// For functions: "pub async fn retrieve(query: &str) -> Result<Vec<CodeChunk>>"
    /// For structs/enums/traits: "pub struct VectorStore", "pub trait Foo: Send + Sync"
    /// Default returns None.
    fn extract_signature(
        &self,
        _source: &str,
        _node: &Node,
        _source_bytes: &[u8],
    ) -> Option<String> {
        None
    }
}
