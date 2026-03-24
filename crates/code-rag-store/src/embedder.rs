use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum EmbedError {
    #[error("failed to initialize embedding model: {0}")]
    Init(#[from] anyhow::Error),

    #[error("embedding generation failed: {0}")]
    Embed(String),
}

/// Wraps fastembed model. Holds loaded model weights in memory.
pub struct Embedder {
    model: TextEmbedding,
    dimension: usize,
}

impl Embedder {
    /// Initialize with BGE-small-en-v1.5 (384 dimensions, good for code)
    pub fn new() -> Result<Self, EmbedError> {
        Self::with_model(EmbeddingModel::BGESmallENV15)
    }

    pub fn with_model(model_name: EmbeddingModel) -> Result<Self, EmbedError> {
        let dimension = embedding_dimension(&model_name);
        let model =
            TextEmbedding::try_new(InitOptions::new(model_name).with_show_download_progress(true))?;

        Ok(Self { model, dimension })
    }

    /// Embed a single text. Convenience wrapper around batch.
    pub fn embed_one(&mut self, text: &str) -> Result<Vec<f32>, EmbedError> {
        self.embed_batch(&[text])
            .map(|mut v| v.pop().unwrap_or_default())
    }

    /// Embed multiple texts in one call (more efficient).
    pub fn embed_batch(&mut self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        self.model
            .embed(texts, None)
            .map_err(|e| EmbedError::Embed(e.to_string()))
    }

    pub fn dimension(&self) -> usize {
        self.dimension
    }
}

fn embedding_dimension(model: &EmbeddingModel) -> usize {
    match model {
        EmbeddingModel::BGESmallENV15 => 384,
        EmbeddingModel::BGEBaseENV15 => 768,
        EmbeddingModel::BGELargeENV15 => 1024,
        EmbeddingModel::AllMiniLML6V2 => 384,
        EmbeddingModel::AllMiniLML12V2 => 384,
        other => panic!("unsupported embedding model: {:?}", other),
    }
}

/// Formats a code chunk for embedding.
/// Concatenates identifier + docstring + code + calls for richer semantic signal.
pub fn format_code_for_embedding(
    identifier: &str,
    language: &str,
    docstring: Option<&str>,
    code: &str,
    calls: &[String],
) -> String {
    let mut parts = vec![format!("{} ({})", identifier, language)];

    if let Some(doc) = docstring
        && !doc.is_empty()
    {
        parts.push(doc.to_string());
    }

    parts.push(code.to_string());

    if !calls.is_empty() {
        parts.push(format!("Calls: {}", calls.join(", ")));
    }

    parts.join("\n")
}

/// Formats a README chunk for embedding.
pub fn format_readme_for_embedding(project_name: &str, content: &str) -> String {
    format!("Project: {}\n{}", project_name, content)
}

/// Formats a crate chunk for embedding.
pub fn format_crate_for_embedding(
    crate_name: &str,
    description: Option<&str>,
    deps: &[String],
) -> String {
    let mut parts = vec![format!("Crate: {}", crate_name)];

    if let Some(desc) = description {
        parts.push(desc.to_string());
    }

    if !deps.is_empty() {
        parts.push(format!("Dependencies: {}", deps.join(", ")));
    }

    parts.join("\n")
}

/// Formats a module doc chunk for embedding.
pub fn format_module_doc_for_embedding(module_name: &str, doc_content: &str) -> String {
    format!("Module: {}\n{}", module_name, doc_content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_code_for_embedding_with_docstring() {
        let result = format_code_for_embedding(
            "process_data",
            "rust",
            Some("Processes input data and returns results"),
            "fn process_data() {}",
            &[],
        );

        assert!(result.contains("process_data (rust)"));
        assert!(result.contains("Processes input data"));
        assert!(result.contains("fn process_data"));
    }

    #[test]
    fn test_format_code_for_embedding_without_docstring() {
        let result = format_code_for_embedding("helper", "python", None, "def helper(): pass", &[]);

        assert!(result.contains("helper (python)"));
        assert!(result.contains("def helper"));
        assert!(!result.contains("\n\n")); // no empty docstring line
    }

    #[test]
    fn test_format_code_with_calls() {
        let result = format_code_for_embedding(
            "foo",
            "rust",
            None,
            "fn foo() {}",
            &["bar".to_string(), "baz".to_string()],
        );

        assert!(result.contains("Calls: bar, baz"));
    }

    #[test]
    fn test_format_code_without_calls() {
        let result = format_code_for_embedding("foo", "rust", None, "fn foo() {}", &[]);

        assert!(!result.contains("Calls:"));
    }

    #[test]
    fn test_format_readme_for_embedding() {
        let result = format_readme_for_embedding("my_project", "# Title\nSome content");

        assert!(result.starts_with("Project: my_project"));
        assert!(result.contains("# Title"));
    }

    // Integration test - only run if model download is acceptable
    #[test]
    #[ignore = "downloads model, run with --ignored"]
    fn test_embedder_produces_correct_dimensions() {
        let mut embedder = Embedder::new().expect("failed to init embedder");
        let embedding = embedder.embed_one("test text").expect("failed to embed");

        assert_eq!(embedding.len(), 384);
        assert_eq!(embedder.dimension(), 384);
    }

    #[test]
    #[ignore = "downloads model, run with --ignored"]
    fn test_embed_batch() {
        let mut embedder = Embedder::new().expect("failed to init embedder");
        let embeddings = embedder
            .embed_batch(&["first", "second", "third"])
            .expect("failed to embed");

        assert_eq!(embeddings.len(), 3);
        assert!(embeddings.iter().all(|e| e.len() == 384));
    }

    #[test]
    #[ignore = "downloads model, run with --ignored"]
    fn test_embed_empty_batch() {
        let mut embedder = Embedder::new().expect("failed to init embedder");
        let embeddings = embedder.embed_batch(&[]).expect("failed to embed");

        assert!(embeddings.is_empty());
    }
}
