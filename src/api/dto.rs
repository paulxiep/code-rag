use serde::{Deserialize, Serialize};

use crate::engine::intent::QueryIntent;
use crate::engine::retriever::{RetrievalResult, ScoredChunk};
use crate::models::{CodeChunk, CrateChunk, ModuleDocChunk, ReadmeChunk};

/// POST /chat request
#[derive(Debug, Deserialize)]
pub struct ChatRequest {
    pub query: String,
}

/// POST /chat response
#[derive(Debug, Serialize)]
pub struct ChatResponse {
    pub answer: String,
    pub sources: Vec<SourceInfo>,
    pub intent: QueryIntent,
}

/// Source reference in response — represents any chunk type.
#[derive(Debug, Serialize, Clone)]
pub struct SourceInfo {
    /// Chunk type discriminator (code, readme, crate, module_doc)
    #[serde(rename = "type")]
    pub chunk_type: String,
    /// File path or crate path
    pub path: String,
    /// Human-readable label (function name, crate name, module name)
    pub label: String,
    /// Parent project
    pub project: String,
    /// Relevance score (0.0–1.0, higher = more relevant)
    pub relevance: f32,
    /// Relevance as integer percentage (for Askama templates)
    pub relevance_pct: u8,
    /// Line number (code chunks only, 0 for others)
    pub line: usize,
}

impl SourceInfo {
    fn from_scored_code(s: &ScoredChunk<CodeChunk>) -> Self {
        Self {
            chunk_type: "code".into(),
            path: s.chunk.file_path.clone(),
            label: s.chunk.identifier.clone(),
            project: s.chunk.project_name.clone(),
            relevance: s.score,
            relevance_pct: (s.score * 100.0).round() as u8,
            line: s.chunk.start_line,
        }
    }

    fn from_scored_readme(s: &ScoredChunk<ReadmeChunk>) -> Self {
        Self {
            chunk_type: "readme".into(),
            path: s.chunk.file_path.clone(),
            label: s.chunk.project_name.clone(),
            project: s.chunk.project_name.clone(),
            relevance: s.score,
            relevance_pct: (s.score * 100.0).round() as u8,
            line: 0,
        }
    }

    fn from_scored_crate(s: &ScoredChunk<CrateChunk>) -> Self {
        Self {
            chunk_type: "crate".into(),
            path: s.chunk.crate_path.clone(),
            label: s.chunk.crate_name.clone(),
            project: s.chunk.project_name.clone(),
            relevance: s.score,
            relevance_pct: (s.score * 100.0).round() as u8,
            line: 0,
        }
    }

    fn from_scored_module_doc(s: &ScoredChunk<ModuleDocChunk>) -> Self {
        Self {
            chunk_type: "module_doc".into(),
            path: s.chunk.file_path.clone(),
            label: s.chunk.module_name.clone(),
            project: s.chunk.project_name.clone(),
            relevance: s.score,
            relevance_pct: (s.score * 100.0).round() as u8,
            line: 0,
        }
    }
}

/// Build sorted source list from all chunk types in a retrieval result.
pub fn build_sources(result: &RetrievalResult) -> Vec<SourceInfo> {
    let mut sources: Vec<SourceInfo> = Vec::new();
    sources.extend(result.code_chunks.iter().map(SourceInfo::from_scored_code));
    sources.extend(
        result
            .readme_chunks
            .iter()
            .map(SourceInfo::from_scored_readme),
    );
    sources.extend(
        result
            .crate_chunks
            .iter()
            .map(SourceInfo::from_scored_crate),
    );
    sources.extend(
        result
            .module_doc_chunks
            .iter()
            .map(SourceInfo::from_scored_module_doc),
    );
    sources.sort_by(|a, b| {
        b.relevance
            .partial_cmp(&a.relevance)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    sources
}

/// GET /health response
#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub version: &'static str,
}

/// GET /projects response
#[derive(Debug, Serialize)]
pub struct ProjectsResponse {
    pub projects: Vec<String>,
    pub count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::intent::QueryIntent;

    fn scored<T>(chunk: T, score: f32) -> ScoredChunk<T> {
        ScoredChunk { chunk, score }
    }

    fn sample_code_chunk() -> CodeChunk {
        CodeChunk {
            file_path: "src/lib.rs".into(),
            language: "rust".into(),
            identifier: "process_data".into(),
            node_type: "function_item".into(),
            code_content: "fn process_data() {}".into(),
            start_line: 42,
            project_name: "my_project".into(),
            docstring: None,
            chunk_id: "test-1".into(),
            content_hash: "hash-1".into(),
            embedding_model_version: "BGESmallENV15_384".into(),
        }
    }

    fn sample_readme_chunk() -> ReadmeChunk {
        ReadmeChunk {
            file_path: "README.md".into(),
            project_name: "my_project".into(),
            content: "# My Project".into(),
            chunk_id: "test-2".into(),
            content_hash: "hash-2".into(),
            embedding_model_version: "BGESmallENV15_384".into(),
        }
    }

    fn sample_crate_chunk() -> CrateChunk {
        CrateChunk {
            crate_name: "my-crate".into(),
            crate_path: "crates/my-crate".into(),
            description: Some("A utility crate".into()),
            dependencies: vec!["types".into()],
            project_name: "my_project".into(),
            chunk_id: "test-3".into(),
            content_hash: "hash-3".into(),
            embedding_model_version: "BGESmallENV15_384".into(),
        }
    }

    fn sample_module_doc_chunk() -> ModuleDocChunk {
        ModuleDocChunk {
            file_path: "src/lib.rs".into(),
            module_name: "my_module".into(),
            doc_content: "Core functionality.".into(),
            project_name: "my_project".into(),
            chunk_id: "test-4".into(),
            content_hash: "hash-4".into(),
            embedding_model_version: "BGESmallENV15_384".into(),
        }
    }

    #[test]
    fn test_source_info_from_code_chunk() {
        let s = SourceInfo::from_scored_code(&scored(sample_code_chunk(), 0.87));
        assert_eq!(s.chunk_type, "code");
        assert_eq!(s.path, "src/lib.rs");
        assert_eq!(s.label, "process_data");
        assert_eq!(s.line, 42);
        assert_eq!(s.relevance_pct, 87);
    }

    #[test]
    fn test_source_info_from_readme_chunk() {
        let s = SourceInfo::from_scored_readme(&scored(sample_readme_chunk(), 0.54));
        assert_eq!(s.chunk_type, "readme");
        assert_eq!(s.label, "my_project");
        assert_eq!(s.line, 0);
    }

    #[test]
    fn test_source_info_from_crate_chunk() {
        let s = SourceInfo::from_scored_crate(&scored(sample_crate_chunk(), 0.72));
        assert_eq!(s.chunk_type, "crate");
        assert_eq!(s.path, "crates/my-crate");
        assert_eq!(s.label, "my-crate");
        assert_eq!(s.line, 0);
    }

    #[test]
    fn test_source_info_from_module_doc_chunk() {
        let s = SourceInfo::from_scored_module_doc(&scored(sample_module_doc_chunk(), 0.65));
        assert_eq!(s.chunk_type, "module_doc");
        assert_eq!(s.label, "my_module");
        assert_eq!(s.line, 0);
    }

    #[test]
    fn test_sources_sorted_by_relevance() {
        let result = RetrievalResult {
            code_chunks: vec![scored(sample_code_chunk(), 0.5)],
            readme_chunks: vec![scored(sample_readme_chunk(), 0.9)],
            crate_chunks: vec![scored(sample_crate_chunk(), 0.3)],
            module_doc_chunks: vec![],
            intent: QueryIntent::Overview,
        };

        let sources = build_sources(&result);
        assert_eq!(sources.len(), 3);
        assert_eq!(sources[0].relevance_pct, 90); // readme
        assert_eq!(sources[1].relevance_pct, 50); // code
        assert_eq!(sources[2].relevance_pct, 30); // crate
    }

    #[test]
    fn test_relevance_pct_computation() {
        let s = SourceInfo::from_scored_code(&scored(sample_code_chunk(), 0.87));
        assert_eq!(s.relevance_pct, 87);

        let s = SourceInfo::from_scored_code(&scored(sample_code_chunk(), 0.0));
        assert_eq!(s.relevance_pct, 0);

        let s = SourceInfo::from_scored_code(&scored(sample_code_chunk(), 1.0));
        assert_eq!(s.relevance_pct, 100);
    }
}
