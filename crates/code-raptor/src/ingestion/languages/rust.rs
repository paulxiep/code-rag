use super::super::language::LanguageHandler;
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

    #[test]
    fn test_rust_doc_pipeline() {
        let handler = RustHandler;
        let source = "/// Pipeline test.\nfn foo() {}";

        let mut analyzer = CodeAnalyzer::new();
        let pairs = analyzer.analyze_with_handler(source, &handler);

        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].0.docstring, Some("Pipeline test.".to_string()));
    }
}
