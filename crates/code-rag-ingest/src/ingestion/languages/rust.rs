use super::super::language::{ImportInfo, LanguageHandler, TypeRelation, collect_type_idents};
use code_rag_types::{EdgeContext, EdgeRelation};
use tree_sitter::{Language, Node, TreeCursor};

pub struct RustHandler;

impl LanguageHandler for RustHandler {
    fn name(&self) -> &'static str {
        "rust"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["rs"]
    }

    fn grammar(&self) -> Language {
        tree_sitter_rust::LANGUAGE.into()
    }

    fn query_string(&self) -> &'static str {
        r#"(function_item name: (identifier) @name) @body
(struct_item name: (type_identifier) @name) @body
(enum_item name: (type_identifier) @name) @body
(trait_item name: (type_identifier) @name) @body
(impl_item type: (type_identifier) @name) @body
(type_item name: (type_identifier) @name) @body
(macro_definition name: (identifier) @name) @body"#
    }

    fn extract_docstring(&self, source: &str, node: &Node, _source_bytes: &[u8]) -> Option<String> {
        // Outer doc comments (///) and #[doc = "..."] attributes.
        // Inner doc (//!) is handled separately by extract_module_docs() in parser.rs.
        let start_line = node.start_position().row;
        if start_line == 0 {
            return None;
        }

        let lines: Vec<&str> = source.lines().collect();
        let mut doc_lines: Vec<String> = Vec::new();

        let mut i = start_line.saturating_sub(1);
        loop {
            let line = lines.get(i).map(|l| l.trim()).unwrap_or("");

            if line.starts_with("///") {
                let content = line
                    .strip_prefix("///")
                    .unwrap_or("")
                    .strip_prefix(' ')
                    .unwrap_or(line.strip_prefix("///").unwrap_or(""));
                doc_lines.push(content.to_string());
            } else if line.starts_with("#[doc") {
                if let Some(start) = line.find('"')
                    && let Some(end) = line.rfind('"')
                    && end > start
                {
                    doc_lines.push(line[start + 1..end].to_string());
                }
            } else if line.starts_with("#[") {
                // Other attributes (#[derive], #[cfg], etc.) — skip but continue scanning
            } else if line.is_empty() {
                if !doc_lines.is_empty() {
                    doc_lines.push(String::new());
                }
            } else {
                break;
            }

            if i == 0 {
                break;
            }
            i -= 1;
        }

        if doc_lines.is_empty() {
            return None;
        }

        doc_lines.reverse();

        // Trim trailing empty lines
        while doc_lines.last().map(|s| s.is_empty()).unwrap_or(false) {
            doc_lines.pop();
        }

        // Trim leading empty lines
        while doc_lines.first().map(|s| s.is_empty()).unwrap_or(false) {
            doc_lines.remove(0);
        }

        if doc_lines.is_empty() {
            None
        } else {
            Some(doc_lines.join("\n"))
        }
    }

    fn extract_calls(&self, _source: &str, node: &Node, source_bytes: &[u8]) -> Vec<String> {
        let mut calls = Vec::new();
        let mut cursor = node.walk();
        collect_calls_recursive(&mut cursor, source_bytes, &mut calls);
        calls.sort();
        calls.dedup();
        calls
    }

    fn extract_signature(&self, source: &str, node: &Node, _source_bytes: &[u8]) -> Option<String> {
        let kind = node.kind();
        match kind {
            "function_item" => {
                let body_block = node.child_by_field_name("body")?;
                let sig_text = &source[node.start_byte()..body_block.start_byte()];
                let sig = sig_text.split_whitespace().collect::<Vec<_>>().join(" ");
                if sig.is_empty() { None } else { Some(sig) }
            }
            "struct_item" | "enum_item" | "trait_item" | "impl_item" | "type_item" => {
                let body = node.child_by_field_name("body").or_else(|| {
                    let mut cursor = node.walk();
                    node.children(&mut cursor)
                        .find(|c| c.kind() == "{" || c.kind() == ";")
                });
                let sig_end = body.map(|b| b.start_byte()).unwrap_or(node.end_byte());
                let sig_text = &source[node.start_byte()..sig_end];
                let sig = sig_text.split_whitespace().collect::<Vec<_>>().join(" ");
                if sig.is_empty() { None } else { Some(sig) }
            }
            _ => None,
        }
    }

    fn extract_file_imports(
        &self,
        _source: &str,
        root: &Node,
        source_bytes: &[u8],
    ) -> Vec<ImportInfo> {
        let mut imports = Vec::new();
        let mut cursor = root.walk();

        for child in root.children(&mut cursor) {
            if child.kind() == "use_declaration" {
                // `pub use` (any visibility modifier on a use) is a re-export.
                let is_reexport = {
                    let mut c = child.walk();
                    child
                        .children(&mut c)
                        .any(|n| n.kind() == "visibility_modifier")
                };
                collect_use_imports(&child, source_bytes, &mut imports, is_reexport);
            }
        }

        imports
    }

    fn extract_type_relations(
        &self,
        _source: &str,
        node: &Node,
        source_bytes: &[u8],
    ) -> Vec<TypeRelation> {
        let mut out = Vec::new();
        match node.kind() {
            // `impl Trait for Type` → Type Implements Trait. The chunk's identifier
            // is `Type` (the `type:` field captured as @name), so the impl chunk is
            // the source and the trait is the target.
            "impl_item" => {
                if let Some(trait_node) = node.child_by_field_name("trait") {
                    push_rust_idents(&trait_node, source_bytes, EdgeRelation::Implements, &mut out);
                }
            }
            // `trait Foo: Bar + Baz` → Foo Extends Bar, Baz (supertrait bounds).
            "trait_item" => {
                if let Some(bounds) = node.child_by_field_name("bounds") {
                    push_rust_idents(&bounds, source_bytes, EdgeRelation::Extends, &mut out);
                }
            }
            // Struct/enum fields → Embeds (composition). Generic args → References.
            "struct_item" | "enum_item" => {
                let mut field_types = Vec::new();
                collect_field_types(node, &mut field_types);
                for ty in &field_types {
                    push_rust_idents(ty, source_bytes, EdgeRelation::Embeds, &mut out);
                }
            }
            // fn params → References(ParameterType); return → References(ReturnType).
            "function_item" => {
                if let Some(params) = node.child_by_field_name("parameters") {
                    let mut c = params.walk();
                    for p in params.children(&mut c) {
                        if let Some(ty) = p.child_by_field_name("type") {
                            push_rust_refs(
                                &ty,
                                source_bytes,
                                EdgeContext::ParameterType,
                                &mut out,
                            );
                        }
                    }
                }
                if let Some(ret) = node.child_by_field_name("return_type") {
                    push_rust_refs(&ret, source_bytes, EdgeContext::ReturnType, &mut out);
                }
            }
            _ => {}
        }
        out
    }
}

/// Collect type identifiers from a type node and push them as `relation` for the
/// head type and `References(GenericArg)` for any generic arguments. Std/container
/// types (Vec, Option, …) are kept here but dropped later at resolution since no
/// project chunk carries that identifier.
fn push_rust_idents(
    type_node: &Node,
    src: &[u8],
    relation: EdgeRelation,
    out: &mut Vec<TypeRelation>,
) {
    for (name, is_generic) in collect_type_idents(type_node, src, &["type_identifier"], &["type_arguments"]) {
        if is_generic {
            out.push(TypeRelation::new(name, EdgeRelation::References, EdgeContext::GenericArg));
        } else {
            out.push(TypeRelation::new(name, relation, EdgeContext::None));
        }
    }
}

/// Like [`push_rust_idents`] but the head type carries a `References` edge with the
/// given positional `context` (parameter/return); generics stay `GenericArg`.
fn push_rust_refs(type_node: &Node, src: &[u8], context: EdgeContext, out: &mut Vec<TypeRelation>) {
    for (name, is_generic) in collect_type_idents(type_node, src, &["type_identifier"], &["type_arguments"]) {
        let ctx = if is_generic { EdgeContext::GenericArg } else { context };
        out.push(TypeRelation::new(name, EdgeRelation::References, ctx));
    }
}

/// Collect the `type:` field node of every `field_declaration` reachable under a
/// struct/enum body (recursively, so enum-variant field lists are covered too).
fn collect_field_types<'a>(node: &Node<'a>, out: &mut Vec<Node<'a>>) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "field_declaration"
            && let Some(ty) = child.child_by_field_name("type")
        {
            out.push(ty);
        }
        collect_field_types(&child, out);
    }
}

/// Extract imports from a `use_declaration` node.
/// Handles: `use crate::module::function;`, `use super::module::*;`,
/// `use crate::module::{foo, bar};`
fn collect_use_imports(
    node: &Node,
    source_bytes: &[u8],
    imports: &mut Vec<ImportInfo>,
    is_reexport: bool,
) {
    // Walk the use_declaration looking for scoped_use_list or a simple use_path
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "use_as_clause" | "scoped_identifier" => {
                // Simple path like `use crate::foo::bar;`
                if let Ok(text) = child.utf8_text(source_bytes) {
                    let text = text.split(" as ").next().unwrap_or(text).trim();
                    if let Some((path, name)) = text.rsplit_once("::") {
                        imports.push(ImportInfo {
                            imported_name: name.to_string(),
                            source_path: path.to_string(),
                            is_reexport,
                        });
                    }
                }
            }
            "scoped_use_list" => {
                collect_scoped_use_list(&child, source_bytes, imports, is_reexport);
            }
            _ => {}
        }
    }
}

/// Handle `use crate::module::{foo, bar};`
fn collect_scoped_use_list(
    node: &Node,
    source_bytes: &[u8],
    imports: &mut Vec<ImportInfo>,
    is_reexport: bool,
) {
    // Find the path prefix (everything before `::{ ... }`)
    let full_text = node.utf8_text(source_bytes).unwrap_or("");

    // The parent path is the scoped_identifier before the `::{`
    let path_prefix = if let Some(prefix_end) = full_text.find("::{") {
        full_text[..prefix_end].trim()
    } else {
        return;
    };

    // Walk children for use_list items
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "use_list" {
            let mut list_cursor = child.walk();
            for item in child.children(&mut list_cursor) {
                match item.kind() {
                    "identifier" => {
                        if let Ok(name) = item.utf8_text(source_bytes) {
                            imports.push(ImportInfo {
                                imported_name: name.to_string(),
                                source_path: path_prefix.to_string(),
                                is_reexport,
                            });
                        }
                    }
                    "use_as_clause" | "scoped_identifier" => {
                        if let Ok(text) = item.utf8_text(source_bytes) {
                            let text = text.split(" as ").next().unwrap_or(text).trim();
                            if let Some((sub_path, name)) = text.rsplit_once("::") {
                                imports.push(ImportInfo {
                                    imported_name: name.to_string(),
                                    source_path: format!("{}::{}", path_prefix, sub_path),
                                    is_reexport,
                                });
                            } else {
                                imports.push(ImportInfo {
                                    imported_name: text.to_string(),
                                    source_path: path_prefix.to_string(),
                                    is_reexport,
                                });
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

/// Walk tree-sitter AST collecting call identifiers.
/// Rust call expressions:
/// - Direct: `call_expression > function: identifier` → `foo()`
/// - Method: `call_expression > function: field_expression > field: field_identifier` → `self.bar()`
fn collect_calls_recursive(cursor: &mut TreeCursor, source_bytes: &[u8], calls: &mut Vec<String>) {
    let node = cursor.node();

    if node.kind() == "call_expression"
        && let Some(func) = node.child_by_field_name("function")
    {
        match func.kind() {
            "identifier" => {
                if let Ok(name) = func.utf8_text(source_bytes) {
                    calls.push(name.to_string());
                }
            }
            "field_expression" => {
                if let Some(field) = func.child_by_field_name("field")
                    && let Ok(name) = field.utf8_text(source_bytes)
                {
                    calls.push(name.to_string());
                }
            }
            "scoped_identifier" => {
                // module::function() — extract the last segment (function name)
                if let Some(name_node) = func.child_by_field_name("name")
                    && let Ok(name) = name_node.utf8_text(source_bytes)
                {
                    calls.push(name.to_string());
                }
            }
            _ => {}
        }
    }

    if cursor.goto_first_child() {
        collect_calls_recursive(cursor, source_bytes, calls);
        while cursor.goto_next_sibling() {
            collect_calls_recursive(cursor, source_bytes, calls);
        }
        cursor.goto_parent();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ingestion::parser::CodeAnalyzer;

    /// Helper: parse source with RustHandler, extract calls from first body node
    fn extract_calls_from(source: &str) -> Vec<String> {
        let handler = RustHandler;
        let mut parser = tree_sitter::Parser::new();
        let grammar = handler.grammar();
        parser.set_language(&grammar).unwrap();
        let tree = parser.parse(source, None).unwrap();

        let query = tree_sitter::Query::new(&grammar, handler.query_string()).unwrap();
        let mut cursor = tree_sitter::QueryCursor::new();
        let body_idx = query.capture_index_for_name("body");

        let source_bytes = source.as_bytes();
        let mut matches = cursor.captures(&query, tree.root_node(), source_bytes);
        use tree_sitter::StreamingIterator;
        if let Some((m, _)) = matches.next() {
            if let Some(body) = m.captures.iter().find(|c| Some(c.index) == body_idx) {
                handler.extract_calls(source, &body.node, source_bytes)
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        }
    }

    /// Helper: parse source with RustHandler, extract docstring from first body node
    fn extract_doc(source: &str) -> Option<String> {
        let handler = RustHandler;
        let mut parser = tree_sitter::Parser::new();
        let grammar = handler.grammar();
        parser.set_language(&grammar).unwrap();
        let tree = parser.parse(source, None).unwrap();

        let query = tree_sitter::Query::new(&grammar, handler.query_string()).unwrap();
        let mut cursor = tree_sitter::QueryCursor::new();
        let body_idx = query.capture_index_for_name("body");

        let source_bytes = source.as_bytes();
        let mut matches = cursor.captures(&query, tree.root_node(), source_bytes);
        use tree_sitter::StreamingIterator;
        if let Some((m, _)) = matches.next() {
            let body = m.captures.iter().find(|c| Some(c.index) == body_idx)?;
            handler.extract_docstring(source, &body.node, source_bytes)
        } else {
            None
        }
    }

    #[test]
    fn test_rust_doc_simple() {
        let doc = extract_doc("/// Calculates the factorial.\nfn factorial() {}");
        assert_eq!(doc, Some("Calculates the factorial.".to_string()));
    }

    #[test]
    fn test_rust_doc_multiline() {
        let doc = extract_doc("/// Line one.\n/// Line two.\nfn foo() {}");
        assert_eq!(doc, Some("Line one.\nLine two.".to_string()));
    }

    #[test]
    fn test_rust_doc_with_sections() {
        let source = "/// Summary.\n///\n/// # Arguments\n///\n/// * `x` - the value\nfn foo() {}";
        let doc = extract_doc(source);
        assert_eq!(
            doc,
            Some("Summary.\n\n# Arguments\n\n* `x` - the value".to_string())
        );
    }

    #[test]
    fn test_rust_doc_with_attributes() {
        let source = "/// Creates a new instance.\n#[derive(Debug, Clone)]\nstruct Config {}";
        let doc = extract_doc(source);
        assert_eq!(doc, Some("Creates a new instance.".to_string()));
    }

    #[test]
    fn test_rust_doc_attr_form() {
        let doc = extract_doc("#[doc = \"Attribute docs.\"]\nfn foo() {}");
        assert_eq!(doc, Some("Attribute docs.".to_string()));
    }

    #[test]
    fn test_rust_no_doc() {
        let doc = extract_doc("fn foo() {}");
        assert_eq!(doc, None);
    }

    // V2.1: Call extraction tests

    #[test]
    fn test_rust_extract_calls_simple() {
        let calls = extract_calls_from("fn foo() { bar(); baz(); }");
        assert_eq!(calls, vec!["bar", "baz"]);
    }

    #[test]
    fn test_rust_extract_calls_method() {
        let calls = extract_calls_from("fn foo() { self.bar(); x.baz(); }");
        assert_eq!(calls, vec!["bar", "baz"]);
    }

    #[test]
    fn test_rust_extract_calls_scoped() {
        let calls = extract_calls_from("fn foo() { module::bar(); std::mem::swap(); }");
        assert_eq!(calls, vec!["bar", "swap"]);
    }

    #[test]
    fn test_rust_extract_calls_nested() {
        let calls = extract_calls_from("fn foo() { bar(baz()); }");
        assert_eq!(calls, vec!["bar", "baz"]);
    }

    #[test]
    fn test_rust_extract_calls_empty() {
        let calls = extract_calls_from("fn foo() { let x = 1; }");
        assert!(calls.is_empty());
    }

    #[test]
    fn test_rust_extract_calls_dedup() {
        let calls = extract_calls_from("fn foo() { bar(); bar(); }");
        assert_eq!(calls, vec!["bar"]);
    }

    // C1: Import extraction tests

    fn extract_imports_from(source: &str) -> Vec<ImportInfo> {
        let handler = RustHandler;
        let mut parser = tree_sitter::Parser::new();
        let grammar = handler.grammar();
        parser.set_language(&grammar).unwrap();
        let tree = parser.parse(source, None).unwrap();
        handler.extract_file_imports(source, &tree.root_node(), source.as_bytes())
    }

    #[test]
    fn test_rust_import_simple() {
        let imports = extract_imports_from("use crate::ingestion::parser;\nfn foo() {}");
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].imported_name, "parser");
        assert_eq!(imports[0].source_path, "crate::ingestion");
    }

    #[test]
    fn test_rust_import_scoped_list() {
        let imports = extract_imports_from("use crate::module::{foo, bar};\nfn baz() {}");
        assert_eq!(imports.len(), 2);
        assert!(imports.iter().any(|i| i.imported_name == "foo"));
        assert!(imports.iter().any(|i| i.imported_name == "bar"));
        assert!(imports.iter().all(|i| i.source_path == "crate::module"));
    }

    #[test]
    fn test_rust_import_none() {
        let imports = extract_imports_from("fn foo() {}");
        assert!(imports.is_empty());
    }

    #[test]
    fn test_rust_doc_pipeline() {
        let handler = RustHandler;
        let source = "/// Pipeline test.\nfn foo() {}";

        let mut analyzer = CodeAnalyzer::new();
        let pairs = analyzer.analyze_with_handler(source, &handler);

        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].0.docstring, Some("Pipeline test.".to_string()));
    }

    // Track R (R1): type-relation extraction

    /// Parse `source`, return type relations from the first matched definition.
    fn type_relations_from(source: &str) -> Vec<TypeRelation> {
        let handler = RustHandler;
        let mut parser = tree_sitter::Parser::new();
        let grammar = handler.grammar();
        parser.set_language(&grammar).unwrap();
        let tree = parser.parse(source, None).unwrap();
        let query = tree_sitter::Query::new(&grammar, handler.query_string()).unwrap();
        let mut cursor = tree_sitter::QueryCursor::new();
        let body_idx = query.capture_index_for_name("body");
        let src = source.as_bytes();
        let mut matches = cursor.captures(&query, tree.root_node(), src);
        use tree_sitter::StreamingIterator;
        if let Some((m, _)) = matches.next() {
            if let Some(b) = m.captures.iter().find(|c| Some(c.index) == body_idx) {
                return handler.extract_type_relations(source, &b.node, src);
            }
        }
        Vec::new()
    }

    #[test]
    fn test_rust_implements() {
        let rels = type_relations_from("impl Embedder for FastEmbedImpl {}");
        assert!(rels.iter().any(|r| r.relation == EdgeRelation::Implements
            && r.target_name == "Embedder"));
    }

    #[test]
    fn test_rust_trait_extends() {
        let rels = type_relations_from("trait Reranker: Send + Sync {}");
        let extends: Vec<_> = rels
            .iter()
            .filter(|r| r.relation == EdgeRelation::Extends)
            .map(|r| r.target_name.as_str())
            .collect();
        assert!(extends.contains(&"Send"));
        assert!(extends.contains(&"Sync"));
    }

    #[test]
    fn test_rust_struct_embeds() {
        let rels = type_relations_from("struct AppState { store: VectorStore, n: usize }");
        assert!(rels.iter().any(|r| r.relation == EdgeRelation::Embeds
            && r.target_name == "VectorStore"));
    }

    #[test]
    fn test_rust_fn_references() {
        let rels = type_relations_from("fn retrieve(ctx: QueryContext) -> RetrievalResult {}");
        assert!(rels.iter().any(|r| r.relation == EdgeRelation::References
            && r.context == EdgeContext::ParameterType
            && r.target_name == "QueryContext"));
        assert!(rels.iter().any(|r| r.relation == EdgeRelation::References
            && r.context == EdgeContext::ReturnType
            && r.target_name == "RetrievalResult"));
    }

    #[test]
    fn test_rust_inherent_impl_no_implements() {
        // `impl Foo {}` (no trait) must not produce an Implements edge.
        let rels = type_relations_from("impl FastEmbedImpl { fn new() {} }");
        assert!(!rels.iter().any(|r| r.relation == EdgeRelation::Implements));
    }
}
