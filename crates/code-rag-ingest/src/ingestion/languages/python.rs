use super::super::language::{ImportInfo, LanguageHandler};
use tree_sitter::{Language, Node, TreeCursor};

pub struct PythonHandler;

impl LanguageHandler for PythonHandler {
    fn name(&self) -> &'static str {
        "python"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["py"]
    }

    fn grammar(&self) -> Language {
        tree_sitter_python::LANGUAGE.into()
    }

    fn query_string(&self) -> &'static str {
        r#"(function_definition name: (identifier) @name) @body
(class_definition name: (identifier) @name) @body"#
    }

    fn extract_docstring(&self, _source: &str, node: &Node, source_bytes: &[u8]) -> Option<String> {
        // Python docstrings are the first expression_statement in the body block
        let body = node.child_by_field_name("body")?;
        let first_stmt = body.named_child(0)?;

        if first_stmt.kind() != "expression_statement" {
            return None;
        }

        let expr = first_stmt.named_child(0)?;
        if expr.kind() != "string" {
            return None;
        }

        let raw = expr.utf8_text(source_bytes).ok()?;
        parse_python_docstring(raw)
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
        match node.kind() {
            "function_definition" | "class_definition" => {
                let body = node.child_by_field_name("body")?;
                let sig_text = &source[node.start_byte()..body.start_byte()];
                let sig = sig_text
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
                    .trim_end_matches(':')
                    .trim()
                    .to_string();
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
            if child.kind() == "import_from_statement" {
                // `from foo.bar import baz, qux`
                let module_name = child
                    .child_by_field_name("module_name")
                    .and_then(|n| n.utf8_text(source_bytes).ok())
                    .unwrap_or("");

                let module_end = child
                    .child_by_field_name("module_name")
                    .map(|n| n.end_byte())
                    .unwrap_or(0);

                let mut name_cursor = child.walk();
                for name_child in child.children(&mut name_cursor) {
                    if (name_child.kind() == "dotted_name" || name_child.kind() == "identifier")
                        && name_child.start_byte() > module_end
                        && let Ok(name) = name_child.utf8_text(source_bytes)
                    {
                        // For dotted_name like `bar.baz`, take the last part
                        let imported = name.rsplit('.').next().unwrap_or(name);
                        imports.push(ImportInfo {
                            imported_name: imported.to_string(),
                            source_path: module_name.to_string(),
                        });
                    }
                }
            }
        }

        imports
    }
}

/// Parse a Python string literal into clean docstring text
fn parse_python_docstring(raw: &str) -> Option<String> {
    let trimmed = raw.trim();

    // Determine quote style and strip delimiters
    let content = if (trimmed.starts_with("\"\"\"") && trimmed.ends_with("\"\"\""))
        || (trimmed.starts_with("'''") && trimmed.ends_with("'''"))
    {
        &trimmed[3..trimmed.len() - 3]
    } else if (trimmed.starts_with('"') && trimmed.ends_with('"')
        || trimmed.starts_with('\'') && trimmed.ends_with('\''))
        && trimmed.len() >= 2
    {
        &trimmed[1..trimmed.len() - 1]
    } else {
        return None;
    };

    let cleaned = dedent_docstring(content);

    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned)
    }
}

/// Remove common leading whitespace from multi-line docstring
fn dedent_docstring(content: &str) -> String {
    let lines: Vec<&str> = content.lines().collect();

    if lines.is_empty() {
        return String::new();
    }

    let first_line = lines[0].trim();

    if lines.len() == 1 {
        return first_line.to_string();
    }

    // Find minimum indentation of non-empty lines (excluding first)
    let min_indent = lines[1..]
        .iter()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.len() - line.trim_start().len())
        .min()
        .unwrap_or(0);

    let mut result = Vec::new();

    if !first_line.is_empty() {
        result.push(first_line.to_string());
    }

    for line in &lines[1..] {
        if line.trim().is_empty() {
            result.push(String::new());
        } else if line.len() >= min_indent {
            result.push(line[min_indent..].to_string());
        } else {
            result.push(line.trim().to_string());
        }
    }

    // Trim trailing empty lines
    while result.last().map(|s| s.is_empty()).unwrap_or(false) {
        result.pop();
    }

    result.join("\n")
}

/// Walk tree-sitter AST collecting call identifiers.
/// Python call expressions:
/// - Direct: `call > function: identifier` → `foo()`
/// - Method: `call > function: attribute > attribute: identifier` → `self.bar()`
fn collect_calls_recursive(cursor: &mut TreeCursor, source_bytes: &[u8], calls: &mut Vec<String>) {
    let node = cursor.node();

    if node.kind() == "call"
        && let Some(func) = node.child_by_field_name("function")
    {
        match func.kind() {
            "identifier" => {
                if let Ok(name) = func.utf8_text(source_bytes) {
                    calls.push(name.to_string());
                }
            }
            "attribute" => {
                if let Some(attr) = func.child_by_field_name("attribute")
                    && let Ok(name) = attr.utf8_text(source_bytes)
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

    #[test]
    fn test_python_doc_simple() {
        let handler = PythonHandler;
        let source = "def foo():\n    \"\"\"Simple docstring.\"\"\"\n    pass";

        let mut analyzer = CodeAnalyzer::new();
        let chunks = chunks_only(analyzer.analyze_with_handler(source, &handler));

        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].docstring, Some("Simple docstring.".to_string()));
    }

    #[test]
    fn test_python_doc_multiline() {
        let handler = PythonHandler;
        let source = "def foo():\n    \"\"\"\n    Line one.\n    Line two.\n    \"\"\"\n    pass";

        let mut analyzer = CodeAnalyzer::new();
        let chunks = chunks_only(analyzer.analyze_with_handler(source, &handler));

        assert_eq!(chunks.len(), 1);
        assert_eq!(
            chunks[0].docstring,
            Some("Line one.\nLine two.".to_string())
        );
    }

    #[test]
    fn test_python_doc_class() {
        let handler = PythonHandler;
        let source = "class Foo:\n    \"\"\"Class docstring.\"\"\"\n    pass";

        let mut analyzer = CodeAnalyzer::new();
        let chunks = chunks_only(analyzer.analyze_with_handler(source, &handler));

        assert!(
            chunks
                .iter()
                .any(|c| c.identifier == "Foo"
                    && c.docstring == Some("Class docstring.".to_string()))
        );
    }

    #[test]
    fn test_python_doc_single_quotes() {
        let handler = PythonHandler;
        let source = "def foo():\n    '''Single quote docs.'''\n    pass";

        let mut analyzer = CodeAnalyzer::new();
        let chunks = chunks_only(analyzer.analyze_with_handler(source, &handler));

        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].docstring, Some("Single quote docs.".to_string()));
    }

    #[test]
    fn test_python_no_doc() {
        let handler = PythonHandler;
        let source = "def foo():\n    x = 1\n    return x";

        let mut analyzer = CodeAnalyzer::new();
        let chunks = chunks_only(analyzer.analyze_with_handler(source, &handler));

        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].docstring, None);
    }

    // V2.1: Call extraction tests

    /// Helper: parse source with PythonHandler, extract calls from first body node
    fn extract_calls_from(source: &str) -> Vec<String> {
        let handler = PythonHandler;
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

    #[test]
    fn test_python_extract_calls_simple() {
        let calls = extract_calls_from("def foo():\n    bar()\n    baz()");
        assert_eq!(calls, vec!["bar", "baz"]);
    }

    #[test]
    fn test_python_extract_calls_attribute() {
        let calls = extract_calls_from("def foo():\n    self.bar()\n    obj.baz()");
        assert_eq!(calls, vec!["bar", "baz"]);
    }

    #[test]
    fn test_python_extract_calls_empty() {
        let calls = extract_calls_from("def foo():\n    x = 1");
        assert!(calls.is_empty());
    }

    #[test]
    fn test_python_extract_calls_dedup() {
        let calls = extract_calls_from("def foo():\n    bar()\n    bar()");
        assert_eq!(calls, vec!["bar"]);
    }

    // C1: Import extraction tests

    fn extract_imports_from(source: &str) -> Vec<ImportInfo> {
        let handler = PythonHandler;
        let mut parser = tree_sitter::Parser::new();
        let grammar = handler.grammar();
        parser.set_language(&grammar).unwrap();
        let tree = parser.parse(source, None).unwrap();
        handler.extract_file_imports(source, &tree.root_node(), source.as_bytes())
    }

    #[test]
    fn test_python_import_from() {
        let imports = extract_imports_from("from utils import normalize_path\ndef foo(): pass");
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].imported_name, "normalize_path");
        assert_eq!(imports[0].source_path, "utils");
    }

    #[test]
    fn test_python_import_from_multiple() {
        let imports = extract_imports_from("from os.path import join, dirname\ndef foo(): pass");
        assert_eq!(imports.len(), 2);
        assert!(imports.iter().any(|i| i.imported_name == "join"));
        assert!(imports.iter().any(|i| i.imported_name == "dirname"));
    }

    #[test]
    fn test_python_no_imports() {
        let imports = extract_imports_from("def foo():\n    pass");
        assert!(imports.is_empty());
    }

    #[test]
    fn test_python_doc_pipeline() {
        let handler = PythonHandler;
        let source =
            "def greet(name):\n    \"\"\"Return a greeting.\"\"\"\n    return f\"Hello {name}\"";

        let mut analyzer = CodeAnalyzer::new();
        let chunks = chunks_only(analyzer.analyze_with_handler(source, &handler));

        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].identifier, "greet");
        assert_eq!(chunks[0].docstring, Some("Return a greeting.".to_string()));
    }
}
