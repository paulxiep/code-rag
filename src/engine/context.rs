use super::retriever::{RetrievalResult, ScoredChunk};
use crate::models::{CrateChunk, ModuleDocChunk};

/// System prompt - instructs the LLM how to behave
pub const SYSTEM_PROMPT: &str = r#"You are a helpful assistant answering questions about a developer's portfolio of coding projects.

Guidelines:
- Use the provided code snippets and documentation to give accurate, specific answers
- Reference project names, file paths, and function names when relevant
- If the context doesn't contain enough information, say so honestly
- Be concise but thorough
- When explaining code, focus on what it does and why, not line-by-line narration"#;

/// Format retrieved chunks into context for the LLM
pub fn build_context(result: &RetrievalResult) -> String {
    let mut sections = Vec::new();

    // Crate overview (architecture-level context)
    if !result.crate_chunks.is_empty() {
        sections.push(format_crate_section(&result.crate_chunks));
    }

    // Module documentation (crate-level docs)
    if !result.module_doc_chunks.is_empty() {
        sections.push(format_module_doc_section(&result.module_doc_chunks));
    }

    // Code chunks
    if !result.code_chunks.is_empty() {
        sections.push(format_code_section(&result.code_chunks));
    }

    // README chunks
    if !result.readme_chunks.is_empty() {
        sections.push(format_readme_section(&result.readme_chunks));
    }

    if sections.is_empty() {
        return "No relevant code or documentation found.".into();
    }

    sections.join("\n\n")
}

fn format_code_section(chunks: &[ScoredChunk<crate::models::CodeChunk>]) -> String {
    let mut out = String::from("## Relevant Code\n");

    for scored in chunks {
        let chunk = &scored.chunk;
        out.push_str(&format!(
            "\n### `{}` in {} ({}:{})\n",
            chunk.identifier, chunk.project_name, chunk.file_path, chunk.start_line,
        ));
        if let Some(ref doc) = chunk.docstring {
            out.push_str(&format!("**Docs:** {}\n", doc));
        }
        out.push_str(&format!(
            "```{}\n{}\n```\n",
            chunk.language,
            chunk.code_content.trim()
        ));
    }

    out
}

fn format_readme_section(chunks: &[ScoredChunk<crate::models::ReadmeChunk>]) -> String {
    let mut out = String::from("## Project Documentation\n");

    for scored in chunks {
        let chunk = &scored.chunk;
        out.push_str(&format!(
            "\n### {}\n{}\n",
            chunk.project_name,
            truncate(&chunk.content, 800)
        ));
    }

    out
}

fn format_crate_section(chunks: &[ScoredChunk<CrateChunk>]) -> String {
    let mut out = String::from("## Crate Structure\n");

    for scored in chunks {
        let chunk = &scored.chunk;
        let desc = chunk
            .description
            .as_deref()
            .unwrap_or("No description available");
        let deps = if chunk.dependencies.is_empty() {
            "none".to_string()
        } else {
            chunk.dependencies.join(", ")
        };

        out.push_str(&format!(
            "\n### Crate `{}` ({})\n**Description:** {}\n**Local dependencies:** {}\n",
            chunk.crate_name, chunk.project_name, desc, deps
        ));
    }

    out
}

fn format_module_doc_section(chunks: &[ScoredChunk<ModuleDocChunk>]) -> String {
    let mut out = String::from("## Module Documentation\n");

    for scored in chunks {
        let chunk = &scored.chunk;
        out.push_str(&format!(
            "\n### Module `{}` ({})\n{}\n",
            chunk.module_name,
            chunk.project_name,
            truncate(&chunk.doc_content, 600)
        ));
    }

    out
}

/// Build the complete prompt sent to the LLM
pub fn build_prompt(query: &str, context: &str) -> String {
    format!(
        "{system}\n\n---\n\n{context}\n\n---\n\n**Question:** {query}",
        system = SYSTEM_PROMPT,
        context = context,
        query = query
    )
}

/// Truncate string to max characters, respecting word boundaries
fn truncate(s: &str, max_chars: usize) -> &str {
    if s.len() <= max_chars {
        return s;
    }

    // Find last space before limit
    s[..max_chars]
        .rfind(' ')
        .map(|idx| &s[..idx])
        .unwrap_or(&s[..max_chars])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::intent::QueryIntent;
    use crate::models::{CodeChunk, CrateChunk, ModuleDocChunk, ReadmeChunk};

    fn scored<T>(chunk: T, score: f32) -> ScoredChunk<T> {
        ScoredChunk { chunk, score }
    }

    fn sample_code_chunk() -> CodeChunk {
        CodeChunk {
            file_path: "my_project/src/lib.rs".into(),
            language: "rust".into(),
            identifier: "process_data".into(),
            node_type: "function_item".into(),
            code_content: "fn process_data(input: &str) -> Result<Output, Error> {\n    // ...\n}"
                .into(),
            start_line: 42,
            project_name: "my_project".into(),
            docstring: None,
            chunk_id: "test-uuid-1".into(),
            content_hash: "test-hash-1".into(),
            embedding_model_version: "BGESmallENV15_384".into(),
        }
    }

    fn sample_readme_chunk() -> ReadmeChunk {
        ReadmeChunk {
            file_path: "my_project/README.md".into(),
            project_name: "my_project".into(),
            content: "# My Project\n\nA data processing library.".into(),
            chunk_id: "test-uuid-2".into(),
            content_hash: "test-hash-2".into(),
            embedding_model_version: "BGESmallENV15_384".into(),
        }
    }

    fn sample_crate_chunk() -> CrateChunk {
        CrateChunk {
            crate_name: "my-crate".into(),
            crate_path: "my_project/crates/my-crate".into(),
            description: Some("A utility crate".into()),
            dependencies: vec!["types".into(), "utils".into()],
            project_name: "my_project".into(),
            chunk_id: "test-uuid-3".into(),
            content_hash: "test-hash-3".into(),
            embedding_model_version: "BGESmallENV15_384".into(),
        }
    }

    fn sample_module_doc_chunk() -> ModuleDocChunk {
        ModuleDocChunk {
            file_path: "my_project/src/lib.rs".into(),
            module_name: "my_module".into(),
            doc_content: "This module provides core functionality.".into(),
            project_name: "my_project".into(),
            chunk_id: "test-uuid-4".into(),
            content_hash: "test-hash-4".into(),
            embedding_model_version: "BGESmallENV15_384".into(),
        }
    }

    #[test]
    fn test_build_context_with_code() {
        let result = RetrievalResult {
            code_chunks: vec![scored(sample_code_chunk(), 0.9)],
            readme_chunks: vec![],
            crate_chunks: vec![],
            module_doc_chunks: vec![],
            intent: QueryIntent::Implementation,
        };

        let context = build_context(&result);

        assert!(context.contains("## Relevant Code"));
        assert!(context.contains("process_data"));
        assert!(context.contains("my_project"));
        assert!(context.contains("```rust"));
    }

    #[test]
    fn test_build_context_with_readme() {
        let result = RetrievalResult {
            code_chunks: vec![],
            readme_chunks: vec![scored(sample_readme_chunk(), 0.8)],
            crate_chunks: vec![],
            module_doc_chunks: vec![],
            intent: QueryIntent::Overview,
        };

        let context = build_context(&result);

        assert!(context.contains("## Project Documentation"));
        assert!(context.contains("My Project"));
    }

    #[test]
    fn test_build_context_with_crates() {
        let result = RetrievalResult {
            code_chunks: vec![],
            readme_chunks: vec![],
            crate_chunks: vec![scored(sample_crate_chunk(), 0.7)],
            module_doc_chunks: vec![],
            intent: QueryIntent::Overview,
        };

        let context = build_context(&result);

        assert!(context.contains("## Crate Structure"));
        assert!(context.contains("my-crate"));
        assert!(context.contains("types, utils"));
    }

    #[test]
    fn test_build_context_with_module_docs() {
        let result = RetrievalResult {
            code_chunks: vec![],
            readme_chunks: vec![],
            crate_chunks: vec![],
            module_doc_chunks: vec![scored(sample_module_doc_chunk(), 0.6)],
            intent: QueryIntent::Implementation,
        };

        let context = build_context(&result);

        assert!(context.contains("## Module Documentation"));
        assert!(context.contains("my_module"));
        assert!(context.contains("core functionality"));
    }

    #[test]
    fn test_build_context_empty() {
        let result = RetrievalResult {
            code_chunks: vec![],
            readme_chunks: vec![],
            crate_chunks: vec![],
            module_doc_chunks: vec![],
            intent: QueryIntent::Implementation,
        };

        let context = build_context(&result);

        assert!(context.contains("No relevant"));
    }

    #[test]
    fn test_build_prompt_structure() {
        let prompt = build_prompt("What does process_data do?", "## Code\n...");

        assert!(prompt.contains(SYSTEM_PROMPT));
        assert!(prompt.contains("## Code"));
        assert!(prompt.contains("What does process_data do?"));
    }

    #[test]
    fn test_build_context_with_docstring() {
        let mut chunk = sample_code_chunk();
        chunk.docstring = Some("Processes input data and returns results.".into());

        let result = RetrievalResult {
            code_chunks: vec![scored(chunk, 0.9)],
            readme_chunks: vec![],
            crate_chunks: vec![],
            module_doc_chunks: vec![],
            intent: QueryIntent::Implementation,
        };

        let context = build_context(&result);

        assert!(context.contains("**Docs:** Processes input data and returns results."));
        assert!(context.contains("```rust"));
    }

    #[test]
    fn test_truncate_short_string() {
        assert_eq!(truncate("hello", 100), "hello");
    }

    #[test]
    fn test_truncate_at_word_boundary() {
        let result = truncate("hello world foo bar", 12);
        assert_eq!(result, "hello world");
    }
}
