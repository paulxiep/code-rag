use code_rag_types::{CodeChunk, CrateChunk, ModuleDocChunk, ReadmeChunk};

use super::intent::QueryIntent;

/// Max chars of code_content to include in rerank text.
/// 1200 chars ≈ 300-400 code tokens, fits in 490-token budget with query overhead.
const RERANK_CODE_CHAR_LIMIT: usize = 1200;

/// Max chars of readme content to include in rerank text.
/// 1500 chars ≈ 300-375 prose tokens, fits in 490-token budget with query overhead.
const RERANK_README_CHAR_LIMIT: usize = 1500;

/// Extract text content from a chunk for cross-encoder reranking.
pub trait RerankText {
    fn rerank_text(&self) -> String;
}

impl RerankText for CodeChunk {
    fn rerank_text(&self) -> String {
        let mut parts = Vec::new();

        // Use signature (with language label) when available, else identifier
        if let Some(ref sig) = self.signature {
            parts.push(format!("{} ({})", sig, self.language));
        } else {
            parts.push(format!("{} ({})", self.identifier, self.language));
        }

        if let Some(ref doc) = self.docstring
            && !doc.is_empty()
        {
            parts.push(doc.clone());
        }
        if self.code_content.len() > RERANK_CODE_CHAR_LIMIT {
            parts.push(format!(
                "{}...",
                truncate_at_char_boundary(&self.code_content, RERANK_CODE_CHAR_LIMIT)
            ));
        } else {
            parts.push(self.code_content.clone());
        }
        parts.join("\n")
    }
}

/// Truncate a string at a safe UTF-8 char boundary at or below `max_bytes`.
fn truncate_at_char_boundary(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

impl RerankText for ReadmeChunk {
    fn rerank_text(&self) -> String {
        let content = if self.content.len() > RERANK_README_CHAR_LIMIT {
            format!("{}...", truncate_at_char_boundary(&self.content, RERANK_README_CHAR_LIMIT))
        } else {
            self.content.clone()
        };
        format!("Project: {}\n{}", self.project_name, content)
    }
}

impl RerankText for CrateChunk {
    fn rerank_text(&self) -> String {
        let mut parts = vec![format!("Crate: {}", self.crate_name)];
        if let Some(ref desc) = self.description {
            parts.push(desc.clone());
        }
        if !self.dependencies.is_empty() {
            parts.push(format!("Dependencies: {}", self.dependencies.join(", ")));
        }
        parts.join("\n")
    }
}

impl RerankText for ModuleDocChunk {
    fn rerank_text(&self) -> String {
        format!("Module: {}\n{}", self.module_name, self.doc_content)
    }
}

/// Convert cross-encoder logit to 0-1 relevance score.
pub fn sigmoid(logit: f32) -> f32 {
    1.0 / (1.0 + (-logit).exp())
}

/// A chunk paired with its relevance score (0.0–1.0, higher = more relevant).
#[derive(Debug, Clone)]
pub struct ScoredChunk<T> {
    pub chunk: T,
    pub score: f32,
}

/// Retrieved context from vector search, with scores and classified intent.
#[derive(Debug)]
pub struct RetrievalResult {
    pub code_chunks: Vec<ScoredChunk<CodeChunk>>,
    pub readme_chunks: Vec<ScoredChunk<ReadmeChunk>>,
    pub crate_chunks: Vec<ScoredChunk<CrateChunk>>,
    pub module_doc_chunks: Vec<ScoredChunk<ModuleDocChunk>>,
    pub intent: QueryIntent,
}

/// Convert L2 distance to relevance score.
/// Maps [0, ∞) → (0, 1]. Zero distance = perfect match (1.0).
pub fn distance_to_relevance(dist: f32) -> f32 {
    1.0 / (1.0 + dist)
}

/// Convert (chunk, distance) pairs into scored chunks.
pub fn to_scored<T>(pairs: Vec<(T, f32)>) -> Vec<ScoredChunk<T>> {
    pairs
        .into_iter()
        .map(|(chunk, dist)| ScoredChunk {
            score: distance_to_relevance(dist),
            chunk,
        })
        .collect()
}

/// Convert (chunk, relevance_score) pairs into scored chunks.
/// Used when scores are already higher=better (e.g., hybrid search results).
pub fn to_scored_relevance<T>(pairs: Vec<(T, f32)>) -> Vec<ScoredChunk<T>> {
    pairs
        .into_iter()
        .map(|(chunk, score)| ScoredChunk { score, chunk })
        .collect()
}

/// Build a RetrievalResult from raw search results (chunk + distance tuples).
pub fn to_retrieval_result(
    code_raw: Vec<(CodeChunk, f32)>,
    readme_raw: Vec<(ReadmeChunk, f32)>,
    crate_raw: Vec<(CrateChunk, f32)>,
    module_doc_raw: Vec<(ModuleDocChunk, f32)>,
    intent: QueryIntent,
) -> RetrievalResult {
    RetrievalResult {
        code_chunks: to_scored(code_raw),
        readme_chunks: to_scored(readme_raw),
        crate_chunks: to_scored(crate_raw),
        module_doc_chunks: to_scored(module_doc_raw),
        intent,
    }
}

/// A chunk flattened across all types, with common provenance fields
/// for both evaluation (harness) and display (API).
#[derive(Debug, Clone)]
pub struct FlatChunk {
    pub chunk_type: String,
    pub file_path: String,
    pub identifier: Option<String>,
    pub project: String,
    pub relevance: f32,
    pub line: Option<usize>,
}

impl RetrievalResult {
    /// Flatten all chunk types into a single sorted list.
    /// Single source of truth — survives new chunk types with only ONE modification point.
    pub fn flatten(&self) -> Vec<FlatChunk> {
        let mut items = Vec::new();
        for s in &self.code_chunks {
            items.push(FlatChunk {
                chunk_type: "code".into(),
                file_path: s.chunk.file_path.clone(),
                identifier: Some(s.chunk.identifier.clone()),
                project: s.chunk.project_name.clone(),
                relevance: s.score,
                line: Some(s.chunk.start_line),
            });
        }
        for s in &self.readme_chunks {
            items.push(FlatChunk {
                chunk_type: "readme".into(),
                file_path: s.chunk.file_path.clone(),
                identifier: None,
                project: s.chunk.project_name.clone(),
                relevance: s.score,
                line: None,
            });
        }
        for s in &self.crate_chunks {
            items.push(FlatChunk {
                chunk_type: "crate".into(),
                file_path: s.chunk.crate_path.clone(),
                identifier: Some(s.chunk.crate_name.clone()),
                project: s.chunk.project_name.clone(),
                relevance: s.score,
                line: None,
            });
        }
        for s in &self.module_doc_chunks {
            items.push(FlatChunk {
                chunk_type: "module_doc".into(),
                file_path: s.chunk.file_path.clone(),
                identifier: Some(s.chunk.module_name.clone()),
                project: s.chunk.project_name.clone(),
                relevance: s.score,
                line: None,
            });
        }
        items.sort_by(|a, b| {
            b.relevance
                .partial_cmp(&a.relevance)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.file_path.cmp(&b.file_path))
        });
        items
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_distance_to_relevance_zero() {
        let score = distance_to_relevance(0.0);
        assert!((score - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_distance_to_relevance_one() {
        let score = distance_to_relevance(1.0);
        assert!((score - 0.5).abs() < 1e-6);
    }

    #[test]
    fn test_distance_to_relevance_large() {
        let score = distance_to_relevance(100.0);
        assert!(score < 0.02);
        assert!(score > 0.0);
    }

    fn make_code_chunk(file: &str, id: &str, project: &str, line: usize) -> CodeChunk {
        CodeChunk {
            file_path: file.to_string(),
            language: "rust".to_string(),
            identifier: id.to_string(),
            node_type: "function_definition".to_string(),
            code_content: "fn test() {}".to_string(),
            start_line: line,
            project_name: project.to_string(),
            docstring: None,
            signature: None,
            chunk_id: "test-id".to_string(),
            content_hash: "test-hash".to_string(),
            embedding_model_version: "test".to_string(),
        }
    }

    fn make_readme_chunk(file: &str, project: &str) -> ReadmeChunk {
        ReadmeChunk {
            file_path: file.to_string(),
            project_name: project.to_string(),
            content: "# README".to_string(),
            chunk_id: "test-id".to_string(),
            content_hash: "test-hash".to_string(),
            embedding_model_version: "test".to_string(),
        }
    }

    fn make_crate_chunk(name: &str, path: &str, project: &str) -> CrateChunk {
        CrateChunk {
            crate_name: name.to_string(),
            crate_path: path.to_string(),
            description: None,
            dependencies: vec![],
            project_name: project.to_string(),
            chunk_id: "test-id".to_string(),
            content_hash: "test-hash".to_string(),
            embedding_model_version: "test".to_string(),
        }
    }

    fn make_module_doc_chunk(file: &str, module: &str, project: &str) -> ModuleDocChunk {
        ModuleDocChunk {
            file_path: file.to_string(),
            module_name: module.to_string(),
            doc_content: "//! Module doc".to_string(),
            project_name: project.to_string(),
            chunk_id: "test-id".to_string(),
            content_hash: "test-hash".to_string(),
            embedding_model_version: "test".to_string(),
        }
    }

    #[test]
    fn test_flatten_sort_by_relevance_desc() {
        let result = RetrievalResult {
            code_chunks: vec![ScoredChunk {
                chunk: make_code_chunk("src/a.rs", "foo", "proj", 10),
                score: 0.5,
            }],
            readme_chunks: vec![ScoredChunk {
                chunk: make_readme_chunk("README.md", "proj"),
                score: 0.9,
            }],
            crate_chunks: vec![ScoredChunk {
                chunk: make_crate_chunk("my-crate", "crates/my-crate", "proj"),
                score: 0.3,
            }],
            module_doc_chunks: vec![ScoredChunk {
                chunk: make_module_doc_chunk("src/lib.rs", "my_crate", "proj"),
                score: 0.7,
            }],
            intent: QueryIntent::Overview,
        };

        let flat = result.flatten();
        assert_eq!(flat.len(), 4);
        assert_eq!(flat[0].chunk_type, "readme"); // 0.9
        assert_eq!(flat[1].chunk_type, "module_doc"); // 0.7
        assert_eq!(flat[2].chunk_type, "code"); // 0.5
        assert_eq!(flat[3].chunk_type, "crate"); // 0.3
    }

    #[test]
    fn test_flatten_tiebreaker_by_file_path() {
        let result = RetrievalResult {
            code_chunks: vec![
                ScoredChunk {
                    chunk: make_code_chunk("src/z.rs", "z", "proj", 1),
                    score: 0.8,
                },
                ScoredChunk {
                    chunk: make_code_chunk("src/a.rs", "a", "proj", 1),
                    score: 0.8,
                },
            ],
            readme_chunks: vec![],
            crate_chunks: vec![],
            module_doc_chunks: vec![],
            intent: QueryIntent::Implementation,
        };

        let flat = result.flatten();
        assert_eq!(flat[0].file_path, "src/a.rs"); // a before z
        assert_eq!(flat[1].file_path, "src/z.rs");
    }

    #[test]
    fn test_flatten_code_has_line() {
        let result = RetrievalResult {
            code_chunks: vec![ScoredChunk {
                chunk: make_code_chunk("src/lib.rs", "foo", "proj", 42),
                score: 0.5,
            }],
            readme_chunks: vec![],
            crate_chunks: vec![],
            module_doc_chunks: vec![],
            intent: QueryIntent::Implementation,
        };

        let flat = result.flatten();
        assert_eq!(flat[0].line, Some(42));
    }

    #[test]
    fn test_flatten_non_code_has_no_line() {
        let result = RetrievalResult {
            code_chunks: vec![],
            readme_chunks: vec![ScoredChunk {
                chunk: make_readme_chunk("README.md", "proj"),
                score: 0.5,
            }],
            crate_chunks: vec![ScoredChunk {
                chunk: make_crate_chunk("c", "crates/c", "proj"),
                score: 0.4,
            }],
            module_doc_chunks: vec![ScoredChunk {
                chunk: make_module_doc_chunk("src/lib.rs", "m", "proj"),
                score: 0.3,
            }],
            intent: QueryIntent::Overview,
        };

        let flat = result.flatten();
        for item in &flat {
            assert_eq!(item.line, None);
        }
    }

    #[test]
    fn test_sigmoid_zero() {
        let s = sigmoid(0.0);
        assert!((s - 0.5).abs() < 1e-6);
    }

    #[test]
    fn test_sigmoid_large_positive() {
        let s = sigmoid(100.0);
        assert!((s - 1.0).abs() < 1e-4);
    }

    #[test]
    fn test_sigmoid_large_negative() {
        let s = sigmoid(-100.0);
        assert!(s < 1e-4);
    }

    #[test]
    fn test_sigmoid_monotonic() {
        assert!(sigmoid(-2.0) < sigmoid(0.0));
        assert!(sigmoid(0.0) < sigmoid(2.0));
    }

    #[test]
    fn test_rerank_text_code_chunk() {
        let chunk = CodeChunk {
            file_path: "src/lib.rs".to_string(),
            language: "rust".to_string(),
            identifier: "retrieve".to_string(),
            node_type: "function_definition".to_string(),
            code_content: "pub fn retrieve() {}".to_string(),
            start_line: 1,
            project_name: "proj".to_string(),
            docstring: Some("Search vector store".to_string()),
            signature: None,
            chunk_id: "id".to_string(),
            content_hash: "hash".to_string(),
            embedding_model_version: "test".to_string(),
        };
        let text = chunk.rerank_text();
        assert!(text.contains("retrieve (rust)"));
        assert!(text.contains("Search vector store"));
        assert!(text.contains("pub fn retrieve()"));
    }

    #[test]
    fn test_rerank_text_code_chunk_no_docstring() {
        let chunk = make_code_chunk("src/a.rs", "foo", "proj", 1);
        let text = chunk.rerank_text();
        assert!(text.contains("foo (rust)"));
        assert!(text.contains("fn test() {}"));
        // No docstring line
        assert_eq!(text.lines().count(), 2);
    }

    #[test]
    fn test_rerank_text_code_chunk_truncation() {
        let long_code = "x".repeat(2000);
        let chunk = CodeChunk {
            file_path: "src/lib.rs".to_string(),
            language: "rust".to_string(),
            identifier: "big_fn".to_string(),
            node_type: "function_definition".to_string(),
            code_content: long_code,
            start_line: 1,
            project_name: "proj".to_string(),
            docstring: None,
            signature: None,
            chunk_id: "id".to_string(),
            content_hash: "hash".to_string(),
            embedding_model_version: "test".to_string(),
        };
        let text = chunk.rerank_text();
        // Should be truncated: "big_fn (rust)\n" + 1200 chars + "..."
        assert!(text.len() < 1300);
        assert!(text.ends_with("..."));
    }

    #[test]
    fn test_rerank_text_readme_chunk() {
        let chunk = make_readme_chunk("README.md", "proj");
        let text = chunk.rerank_text();
        assert!(text.contains("Project: proj"));
        assert!(text.contains("# README"));
    }

    #[test]
    fn test_rerank_text_readme_chunk_truncation() {
        let long_content = "y".repeat(3000);
        let chunk = ReadmeChunk {
            file_path: "README.md".to_string(),
            project_name: "proj".to_string(),
            content: long_content,
            chunk_id: "id".to_string(),
            content_hash: "hash".to_string(),
            embedding_model_version: "test".to_string(),
        };
        let text = chunk.rerank_text();
        assert!(text.len() < 1600);
        assert!(text.ends_with("..."));
    }

    #[test]
    fn test_rerank_text_crate_chunk() {
        let chunk = CrateChunk {
            crate_name: "engine".to_string(),
            crate_path: "crates/engine".to_string(),
            description: Some("RAG pipeline".to_string()),
            dependencies: vec!["types".to_string(), "store".to_string()],
            project_name: "proj".to_string(),
            chunk_id: "id".to_string(),
            content_hash: "hash".to_string(),
            embedding_model_version: "test".to_string(),
        };
        let text = chunk.rerank_text();
        assert!(text.contains("Crate: engine"));
        assert!(text.contains("RAG pipeline"));
        assert!(text.contains("Dependencies: types, store"));
    }

    #[test]
    fn test_rerank_text_crate_chunk_no_desc() {
        let chunk = make_crate_chunk("c", "crates/c", "proj");
        let text = chunk.rerank_text();
        assert!(text.contains("Crate: c"));
        assert_eq!(text.lines().count(), 1); // No description, no deps
    }

    #[test]
    fn test_rerank_text_module_doc_chunk() {
        let chunk = make_module_doc_chunk("src/lib.rs", "my_crate", "proj");
        let text = chunk.rerank_text();
        assert!(text.contains("Module: my_crate"));
        assert!(text.contains("//! Module doc"));
    }

    #[test]
    fn test_flatten_empty() {
        let result = RetrievalResult {
            code_chunks: vec![],
            readme_chunks: vec![],
            crate_chunks: vec![],
            module_doc_chunks: vec![],
            intent: QueryIntent::Overview,
        };

        let flat = result.flatten();
        assert!(flat.is_empty());
    }
}
