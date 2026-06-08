use code_rag_types::{EdgeContext, EdgeRelation};
use tree_sitter::{Language, Node};

/// An import found in a source file. Used for tier-2 (import-based) edge resolution.
/// Local to code-rag-ingest; not stored in LanceDB.
#[derive(Debug, Clone, Default)]
pub struct ImportInfo {
    /// The imported symbol name, e.g. "normalize_path"
    pub imported_name: String,
    /// The source module path, e.g. "crate::ingestion::mod" or "./utils"
    pub source_path: String,
    /// Track R (R1): true when this is a *re-export* (`pub use` / `export … from`)
    /// rather than a private import — drives `ReExports` vs `Imports` graph edges.
    pub is_reexport: bool,
}

impl ImportInfo {
    /// Private import (`is_reexport = false`).
    pub fn import(imported_name: impl Into<String>, source_path: impl Into<String>) -> Self {
        Self {
            imported_name: imported_name.into(),
            source_path: source_path.into(),
            is_reexport: false,
        }
    }
}

/// Track R (R1): a raw type relation extracted from a definition node, before
/// target resolution. `target_name` is the referenced type/trait identifier; the
/// orchestrator resolves it to a chunk id (reusing call-edge resolution) and emits
/// a `GraphEdge`. `relation` is one of `Implements` / `Extends` / `Embeds` /
/// `References`; `context` is meaningful only for `References`. Local to
/// code-rag-ingest; not stored in LanceDB.
#[derive(Debug, Clone, PartialEq)]
pub struct TypeRelation {
    pub target_name: String,
    pub relation: EdgeRelation,
    pub context: EdgeContext,
}

impl TypeRelation {
    pub fn new(target_name: impl Into<String>, relation: EdgeRelation, context: EdgeContext) -> Self {
        Self {
            target_name: target_name.into(),
            relation,
            context,
        }
    }
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

    /// Track R (R1): extract typed structural relations from a definition node.
    ///
    /// `node` is the `@body` capture (the full definition: impl/trait/struct/class/
    /// function/…). Returns raw `(target_name, relation, context)` triples that the
    /// orchestrator resolves to chunk ids. Mirrors how `extract_calls` /
    /// `extract_file_imports` were added — default returns empty so a language opts
    /// in by overriding. Per-language coverage:
    /// - Rust: `impl Trait for T` → Implements; trait supertrait bounds → Extends;
    ///   struct field types → Embeds; fn param/return/generic types → References.
    /// - Python: base classes → Extends; annotations → References.
    /// - TypeScript: `implements` → Implements; `extends` → Extends; annotations →
    ///   References.
    /// - Go: struct embedding → Embeds; param/return/field types → References
    ///   (interface satisfaction is structural/implicit → no Implements).
    fn extract_type_relations(
        &self,
        _source: &str,
        _node: &Node,
        _source_bytes: &[u8],
    ) -> Vec<TypeRelation> {
        Vec::new()
    }
}

/// Shared helper: collect type-identifier names from a type node, distinguishing
/// the head type from generic arguments. Returns `(name, is_generic_arg)` pairs.
/// Used by per-language `extract_type_relations` to turn `Vec<CodeChunk>` into
/// `[(Vec, false), (CodeChunk, true)]`. Walks the subtree collecting any node whose
/// kind is in `ident_kinds` (e.g. `type_identifier` for Rust/TS).
pub(crate) fn collect_type_idents(
    node: &Node,
    source_bytes: &[u8],
    ident_kinds: &[&str],
    generic_kinds: &[&str],
) -> Vec<(String, bool)> {
    let mut out = Vec::new();
    collect_type_idents_inner(node, source_bytes, ident_kinds, generic_kinds, false, &mut out);
    out
}

/// Track R (R1): scan the comment lines immediately preceding a definition for
/// `NOTE:` / `WHY:` / `HACK:` rationale markers, returning identifier-like tokens
/// mentioned there. High-precision: only CamelCase or snake_case tokens (len ≥ 4)
/// qualify, and target resolution further drops any that aren't project symbols —
/// so a comment like `// WHY: needed because Reranker stalls` yields `Reranker`
/// but not prose words. Language-agnostic: keys off the marker, not comment syntax.
pub(crate) fn extract_rationale_targets(source: &str, def_start_row: usize) -> Vec<String> {
    if def_start_row == 0 {
        return Vec::new();
    }
    let lines: Vec<&str> = source.lines().collect();
    let mut found = Vec::new();
    let mut i = def_start_row; // 0-based row of the definition's first line
    while i > 0 {
        i -= 1;
        let line = lines.get(i).map(|l| l.trim()).unwrap_or("");
        if line.is_empty() {
            continue;
        }
        let is_comment = line.starts_with("//")
            || line.starts_with('#')
            || line.starts_with('*')
            || line.starts_with("/*");
        if !is_comment {
            break; // hit code — stop scanning the comment block
        }
        let upper = line.to_uppercase();
        if upper.contains("NOTE:") || upper.contains("WHY:") || upper.contains("HACK:") {
            for tok in line.split(|c: char| !(c.is_alphanumeric() || c == '_')) {
                let is_identifier_like = tok.len() >= 4
                    && tok.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_')
                    && (tok.contains('_') || tok.chars().any(|c| c.is_uppercase()));
                if is_identifier_like {
                    found.push(tok.to_string());
                }
            }
        }
    }
    found
}

/// Shared helper: collect all descendant nodes whose kind is in `kinds` (the node
/// itself is not matched). Used to find heritage clauses that may be nested under a
/// wrapper node (e.g. TS `class_heritage`).
pub(crate) fn collect_nodes_by_kind<'a>(node: &Node<'a>, kinds: &[&str]) -> Vec<Node<'a>> {
    let mut out = Vec::new();
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if kinds.contains(&child.kind()) {
            out.push(child);
        }
        out.extend(collect_nodes_by_kind(&child, kinds));
    }
    out
}

fn collect_type_idents_inner(
    node: &Node,
    source_bytes: &[u8],
    ident_kinds: &[&str],
    generic_kinds: &[&str],
    in_generic: bool,
    out: &mut Vec<(String, bool)>,
) {
    // Check the node itself first, so a bare type node (e.g. `VectorStore`, which
    // *is* a `type_identifier` with no relevant children) is captured.
    if ident_kinds.contains(&node.kind())
        && let Ok(name) = node.utf8_text(source_bytes)
    {
        out.push((name.to_string(), in_generic));
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        // Once inside a generic-argument node, mark descendants as generic args.
        let child_in_generic = in_generic || generic_kinds.contains(&child.kind());
        collect_type_idents_inner(
            &child,
            source_bytes,
            ident_kinds,
            generic_kinds,
            child_in_generic,
            out,
        );
    }
}
