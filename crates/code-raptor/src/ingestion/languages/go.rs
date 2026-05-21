use super::super::language::{ImportInfo, LanguageHandler};
use tree_sitter::{Language, Node, TreeCursor};

pub struct GoHandler;

impl LanguageHandler for GoHandler {
    fn name(&self) -> &'static str {
        "go"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["go"]
    }

    fn grammar(&self) -> Language {
        tree_sitter_go::LANGUAGE.into()
    }

    fn query_string(&self) -> &'static str {
        r#"(function_declaration name: (identifier) @name) @body
(method_declaration name: (field_identifier) @name) @body
(type_declaration (type_spec name: (type_identifier) @name)) @body
(type_declaration (type_alias name: (type_identifier) @name)) @body"#
    }

    fn extract_docstring(&self, source: &str, node: &Node, _source_bytes: &[u8]) -> Option<String> {
        let start_line = node.start_position().row;
        if start_line == 0 {
            return None;
        }

        let lines: Vec<&str> = source.lines().collect();
        let mut doc_lines: Vec<String> = Vec::new();

        let mut i = start_line.saturating_sub(1);
        loop {
            let line = lines.get(i).map(|l| l.trim()).unwrap_or("");

            if line.starts_with("//go:") {
                // Build/generate directive — skip but continue scanning upward.
            } else if line.starts_with("//") {
                let content = line
                    .strip_prefix("//")
                    .unwrap_or("")
                    .strip_prefix(' ')
                    .unwrap_or(line.strip_prefix("//").unwrap_or(""));
                doc_lines.push(content.to_string());
            } else if line.is_empty() {
                // Blank between doc and decl breaks association in Go convention.
                break;
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

        while doc_lines.last().map(|s| s.is_empty()).unwrap_or(false) {
            doc_lines.pop();
        }
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
            "function_declaration" | "method_declaration" => {
                let body = node.child_by_field_name("body")?;
                let sig_text = &source[node.start_byte()..body.start_byte()];
                let sig = sig_text.split_whitespace().collect::<Vec<_>>().join(" ");
                if sig.is_empty() { None } else { Some(sig) }
            }
            "type_declaration" => {
                let mut cursor = node.walk();
                let brace_byte = node
                    .children(&mut cursor)
                    .find_map(|c| find_first_open_brace(&c));
                let sig_end = brace_byte.unwrap_or(node.end_byte());
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
            if child.kind() == "import_declaration" {
                collect_import_decl(&child, source_bytes, &mut imports);
            }
        }

        imports
    }
}

/// Walk down a subtree looking for the first `{` token (start of a struct/interface body).
fn find_first_open_brace(node: &Node) -> Option<usize> {
    if node.kind() == "{" {
        return Some(node.start_byte());
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(b) = find_first_open_brace(&child) {
            return Some(b);
        }
    }
    None
}

/// Process a single `import_declaration` node, handling both single-import and grouped forms.
fn collect_import_decl(node: &Node, source_bytes: &[u8], imports: &mut Vec<ImportInfo>) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "import_spec" => collect_import_spec(&child, source_bytes, imports),
            "import_spec_list" => {
                let mut list_cursor = child.walk();
                for spec in child.children(&mut list_cursor) {
                    if spec.kind() == "import_spec" {
                        collect_import_spec(&spec, source_bytes, imports);
                    }
                }
            }
            _ => {}
        }
    }
}

/// Extract one ImportInfo from an `import_spec`. Skips blank (`_`) and dot (`.`) imports.
fn collect_import_spec(node: &Node, source_bytes: &[u8], imports: &mut Vec<ImportInfo>) {
    let path_node = match node.child_by_field_name("path") {
        Some(p) => p,
        None => return,
    };
    let raw_path = match path_node.utf8_text(source_bytes) {
        Ok(s) => s,
        Err(_) => return,
    };
    let path = raw_path.trim_matches(|c| c == '"' || c == '`').to_string();

    let alias = node
        .child_by_field_name("name")
        .and_then(|n| n.utf8_text(source_bytes).ok());

    if let Some(a) = alias {
        if a == "_" || a == "." {
            return;
        }
        imports.push(ImportInfo {
            imported_name: a.to_string(),
            source_path: path,
        });
    } else {
        let last_segment = path.rsplit('/').next().unwrap_or(&path).to_string();
        if last_segment.is_empty() {
            return;
        }
        imports.push(ImportInfo {
            imported_name: last_segment,
            source_path: path,
        });
    }
}

/// Walk tree-sitter AST collecting call identifiers.
/// Go call expressions:
/// - Direct: `call_expression > function: identifier` → `foo()`
/// - Method / package-qualified: `call_expression > function: selector_expression > field: field_identifier`
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
            "selector_expression" => {
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

    fn chunks_only(
        pairs: Vec<(code_rag_types::CodeChunk, Vec<String>)>,
    ) -> Vec<code_rag_types::CodeChunk> {
        pairs.into_iter().map(|(c, _)| c).collect()
    }

    /// Helper: parse source with GoHandler and run extract_docstring on the first captured body.
    fn extract_doc(source: &str) -> Option<String> {
        let handler = GoHandler;
        let mut parser = tree_sitter::Parser::new();
        let grammar = handler.grammar();
        parser.set_language(&grammar).unwrap();
        let tree = parser.parse(source, None).unwrap();

        let query = tree_sitter::Query::new(&grammar, handler.query_string()).unwrap();
        let mut cursor = tree_sitter::QueryCursor::new();
        let body_idx = query.capture_index_for_name("body");
        let source_bytes = source.as_bytes();

        use tree_sitter::StreamingIterator;
        let mut matches = cursor.captures(&query, tree.root_node(), source_bytes);
        if let Some((m, _)) = matches.next()
            && let Some(body) = m.captures.iter().find(|c| Some(c.index) == body_idx)
        {
            return handler.extract_docstring(source, &body.node, source_bytes);
        }
        None
    }

    fn extract_calls_from(source: &str) -> Vec<String> {
        let handler = GoHandler;
        let mut parser = tree_sitter::Parser::new();
        let grammar = handler.grammar();
        parser.set_language(&grammar).unwrap();
        let tree = parser.parse(source, None).unwrap();

        let query = tree_sitter::Query::new(&grammar, handler.query_string()).unwrap();
        let mut cursor = tree_sitter::QueryCursor::new();
        let body_idx = query.capture_index_for_name("body");
        let source_bytes = source.as_bytes();

        use tree_sitter::StreamingIterator;
        let mut matches = cursor.captures(&query, tree.root_node(), source_bytes);
        if let Some((m, _)) = matches.next()
            && let Some(body) = m.captures.iter().find(|c| Some(c.index) == body_idx)
        {
            return handler.extract_calls(source, &body.node, source_bytes);
        }
        Vec::new()
    }

    fn extract_signature_from(source: &str) -> Option<String> {
        let handler = GoHandler;
        let mut parser = tree_sitter::Parser::new();
        let grammar = handler.grammar();
        parser.set_language(&grammar).unwrap();
        let tree = parser.parse(source, None).unwrap();

        let query = tree_sitter::Query::new(&grammar, handler.query_string()).unwrap();
        let mut cursor = tree_sitter::QueryCursor::new();
        let body_idx = query.capture_index_for_name("body");
        let source_bytes = source.as_bytes();

        use tree_sitter::StreamingIterator;
        let mut matches = cursor.captures(&query, tree.root_node(), source_bytes);
        if let Some((m, _)) = matches.next()
            && let Some(body) = m.captures.iter().find(|c| Some(c.index) == body_idx)
        {
            return handler.extract_signature(source, &body.node, source_bytes);
        }
        None
    }

    fn extract_imports_from(source: &str) -> Vec<ImportInfo> {
        let handler = GoHandler;
        let mut parser = tree_sitter::Parser::new();
        let grammar = handler.grammar();
        parser.set_language(&grammar).unwrap();
        let tree = parser.parse(source, None).unwrap();
        handler.extract_file_imports(source, &tree.root_node(), source.as_bytes())
    }

    // ------ Docstring tests ------

    #[test]
    fn test_go_doc_single_line() {
        let doc = extract_doc(
            "// Add returns the sum of a and b.\nfunc Add(a, b int) int { return a + b }\n",
        );
        assert_eq!(doc, Some("Add returns the sum of a and b.".to_string()));
    }

    #[test]
    fn test_go_doc_multiline() {
        let src = "// User represents an authenticated user.\n// It is safe for concurrent use.\ntype User struct { ID int }\n";
        let doc = extract_doc(src);
        assert_eq!(
            doc,
            Some(
                "User represents an authenticated user.\nIt is safe for concurrent use."
                    .to_string()
            )
        );
    }

    #[test]
    fn test_go_doc_skips_directive() {
        let src = "// Color is an RGB triple.\n//go:generate stringer -type=Color\ntype Color struct { R, G, B uint8 }\n";
        let doc = extract_doc(src);
        assert_eq!(doc, Some("Color is an RGB triple.".to_string()));
    }

    #[test]
    fn test_go_doc_none() {
        let doc = extract_doc("func Bare() {}\n");
        assert_eq!(doc, None);
    }

    #[test]
    fn test_go_doc_blank_line_breaks_association() {
        let src = "// Not associated.\n\nfunc Foo() {}\n";
        let doc = extract_doc(src);
        assert_eq!(doc, None);
    }

    #[test]
    fn test_go_doc_method() {
        let src = "// Save persists the user.\nfunc (u *User) Save() error { return nil }\n";
        let doc = extract_doc(src);
        assert_eq!(doc, Some("Save persists the user.".to_string()));
    }

    // ------ Call extraction tests ------

    #[test]
    fn test_go_extract_calls_simple() {
        let calls = extract_calls_from("func run() { foo(); bar(); foo() }\n");
        assert_eq!(calls, vec!["bar", "foo"]);
    }

    #[test]
    fn test_go_extract_calls_selector() {
        let calls =
            extract_calls_from("func run() { fmt.Println(\"hi\"); u.Save(); log.Printf(\"x\") }\n");
        assert_eq!(calls, vec!["Printf", "Println", "Save"]);
    }

    #[test]
    fn test_go_extract_calls_empty() {
        let calls = extract_calls_from("func empty() { x := 1; _ = x }\n");
        assert!(calls.is_empty());
    }

    // ------ Signature tests ------

    #[test]
    fn test_go_signature_function() {
        let sig = extract_signature_from(
            "func Greet(name string) (string, error) { return \"\", nil }\n",
        );
        assert_eq!(
            sig,
            Some("func Greet(name string) (string, error)".to_string())
        );
    }

    #[test]
    fn test_go_signature_method() {
        let sig = extract_signature_from(
            "func (u *User) Save(ctx context.Context) error { return nil }\n",
        );
        assert_eq!(
            sig,
            Some("func (u *User) Save(ctx context.Context) error".to_string())
        );
    }

    #[test]
    fn test_go_signature_struct() {
        let sig = extract_signature_from("type Server struct { addr string }\n");
        assert_eq!(sig, Some("type Server struct".to_string()));
    }

    #[test]
    fn test_go_signature_interface() {
        let sig = extract_signature_from("type Handler interface { ServeHTTP() }\n");
        assert_eq!(sig, Some("type Handler interface".to_string()));
    }

    #[test]
    fn test_go_signature_type_alias() {
        let sig = extract_signature_from("type UserID int\n");
        assert_eq!(sig, Some("type UserID int".to_string()));
    }

    // ------ Import tests ------

    #[test]
    fn test_go_import_single() {
        let imports = extract_imports_from("package main\nimport \"fmt\"\nfunc main() {}\n");
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].imported_name, "fmt");
        assert_eq!(imports[0].source_path, "fmt");
    }

    #[test]
    fn test_go_import_grouped_with_alias() {
        let src = "package main\nimport (\n    \"fmt\"\n    \"encoding/json\"\n    log \"github.com/sirupsen/logrus\"\n)\nfunc main() {}\n";
        let imports = extract_imports_from(src);
        assert_eq!(imports.len(), 3);
        assert!(
            imports
                .iter()
                .any(|i| i.imported_name == "fmt" && i.source_path == "fmt")
        );
        assert!(
            imports
                .iter()
                .any(|i| i.imported_name == "json" && i.source_path == "encoding/json")
        );
        assert!(
            imports
                .iter()
                .any(|i| i.imported_name == "log" && i.source_path == "github.com/sirupsen/logrus")
        );
    }

    #[test]
    fn test_go_import_blank_and_dot_skipped() {
        let src = "package main\nimport (\n    _ \"github.com/lib/pq\"\n    . \"math\"\n)\nfunc main() {}\n";
        let imports = extract_imports_from(src);
        assert!(imports.is_empty());
    }

    #[test]
    fn test_go_no_imports() {
        let imports = extract_imports_from("package main\nfunc main() {}\n");
        assert!(imports.is_empty());
    }

    // ------ End-to-end pipeline ------

    #[test]
    fn test_go_pipeline() {
        let handler = GoHandler;
        let source = "// Greet returns a greeting.\nfunc Greet(name string) string {\n    return fmt.Sprintf(\"Hello %s\", name)\n}\n";

        let mut analyzer = CodeAnalyzer::new();
        let pairs = analyzer.analyze_with_handler(source, &handler);
        let calls = pairs[0].1.clone();
        let chunks = chunks_only(pairs);

        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].identifier, "Greet");
        assert_eq!(chunks[0].language, "go");
        assert_eq!(
            chunks[0].docstring,
            Some("Greet returns a greeting.".to_string())
        );
        assert_eq!(
            chunks[0].signature,
            Some("func Greet(name string) string".to_string())
        );
        assert!(calls.contains(&"Sprintf".to_string()));
    }
}
