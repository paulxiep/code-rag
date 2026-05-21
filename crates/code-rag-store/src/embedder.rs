use std::sync::Mutex;

use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use thiserror::Error;
use tracing::warn;

use crate::seams;

#[derive(Error, Debug, serde::Serialize, serde::Deserialize)]
pub enum EmbedError {
    #[error("failed to initialize embedding model: {0}")]
    Init(String),

    #[error("embedding generation failed: {0}")]
    Embed(String),

    #[error("embedder mutex poisoned (a prior call panicked)")]
    Poisoned,
}

impl From<anyhow::Error> for EmbedError {
    fn from(e: anyhow::Error) -> Self {
        warn!(error = format!("{e:#}"), "embedder init failed");
        EmbedError::Init(e.to_string())
    }
}

/// Concrete fastembed-backed `Embedder` seam impl.
///
/// fastembed's `TextEmbedding` is `!Sync` and its `embed*` methods take
/// `&mut self`; we wrap it in `std::sync::Mutex` here so the seam trait
/// methods can be `&self` (the locking is an implementation detail invisible
/// to call sites going through `client::<dyn Embedder>()`). The mutex is
/// `std::sync` rather than `tokio::sync` because no `.await` happens while
/// the lock is held.
pub struct FastEmbedImpl {
    model: Mutex<TextEmbedding>,
    dimension: usize,
}

impl FastEmbedImpl {
    /// Initialize with BGE-small-en-v1.5 (384 dimensions, good for code).
    pub fn new() -> Result<Self, EmbedError> {
        Self::with_model(EmbeddingModel::BGESmallENV15)
    }

    pub fn with_model(model_name: EmbeddingModel) -> Result<Self, EmbedError> {
        let dimension = embedding_dimension(&model_name);
        let model =
            TextEmbedding::try_new(InitOptions::new(model_name).with_show_download_progress(true))?;

        Ok(Self {
            model: Mutex::new(model),
            dimension,
        })
    }
}

impl seams::Embedder for FastEmbedImpl {
    fn embed_one(&self, text: &str) -> Result<Vec<f32>, EmbedError> {
        self.embed_batch(&[text])
            .map(|mut v| v.pop().unwrap_or_default())
    }

    fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        let mut guard = self.model.lock().map_err(|_| EmbedError::Poisoned)?;
        guard
            .embed(texts, None)
            .map_err(|e| EmbedError::Embed(e.to_string()))
    }

    fn dimension(&self) -> usize {
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
/// Concatenates identifier/signature + docstring + code + calls for richer semantic signal.
/// When a signature is available, it replaces the bare identifier line.
pub fn format_code_for_embedding(
    identifier: &str,
    language: &str,
    docstring: Option<&str>,
    code: &str,
    calls: &[String],
    signature: Option<&str>,
) -> String {
    let mut parts = Vec::new();

    // Signature (with language label) when available, else identifier
    if let Some(sig) = signature {
        parts.push(format!("{} ({})", sig, language));
    } else {
        parts.push(format!("{} ({})", identifier, language));
    }

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

/// Formats a signature chunk for embedding (B5 dual-embedding path).
///
/// Short, high-density: signature line + optional docstring, with the
/// language label for parity with `format_code_for_embedding`. Returns
/// `None` when the chunk has no signature (macros, statements, etc.).
pub fn format_signature_for_embedding(
    signature: Option<&str>,
    language: &str,
    docstring: Option<&str>,
) -> Option<String> {
    let sig = signature?;
    let mut parts = vec![format!("{} ({})", sig, language)];
    if let Some(doc) = docstring
        && !doc.is_empty()
    {
        parts.push(doc.to_string());
    }
    Some(parts.join("\n"))
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
    fn embed_error_serde_roundtrip() {
        for err in [
            EmbedError::Init("boom".into()),
            EmbedError::Embed("oops".into()),
            EmbedError::Poisoned,
        ] {
            let json = serde_json::to_string(&err).unwrap();
            let back: EmbedError = serde_json::from_str(&json).unwrap();
            assert_eq!(format!("{back:?}"), format!("{err:?}"));
        }
    }

    #[test]
    fn test_format_code_for_embedding_with_docstring() {
        let result = format_code_for_embedding(
            "process_data",
            "rust",
            Some("Processes input data and returns results"),
            "fn process_data() {}",
            &[],
            None,
        );

        assert!(result.contains("process_data (rust)"));
        assert!(result.contains("Processes input data"));
        assert!(result.contains("fn process_data"));
    }

    #[test]
    fn test_format_code_for_embedding_without_docstring() {
        let result =
            format_code_for_embedding("helper", "python", None, "def helper(): pass", &[], None);

        assert!(result.contains("helper (python)"));
        assert!(result.contains("def helper"));
        assert!(!result.contains("\n\n")); // no empty docstring line
    }

    #[test]
    fn test_format_code_with_signature() {
        let result = format_code_for_embedding(
            "foo",
            "rust",
            None,
            "fn foo() {}",
            &[],
            Some("pub fn foo() -> bool"),
        );

        assert!(result.contains("pub fn foo() -> bool (rust)"));
        assert!(!result.contains("\nfoo (rust)")); // signature replaces identifier
    }

    #[test]
    fn test_format_code_with_calls() {
        let result = format_code_for_embedding(
            "foo",
            "rust",
            None,
            "fn foo() {}",
            &["bar".to_string(), "baz".to_string()],
            None,
        );

        assert!(result.contains("Calls: bar, baz"));
    }

    #[test]
    fn test_format_code_without_calls() {
        let result = format_code_for_embedding("foo", "rust", None, "fn foo() {}", &[], None);

        assert!(!result.contains("Calls:"));
    }

    #[test]
    fn test_format_readme_for_embedding() {
        let result = format_readme_for_embedding("my_project", "# Title\nSome content");

        assert!(result.starts_with("Project: my_project"));
        assert!(result.contains("# Title"));
    }

    #[test]
    fn test_format_signature_for_embedding_with_docstring() {
        let result = format_signature_for_embedding(
            Some("pub fn parse(input: &str) -> Result<Ast, ParseError>"),
            "rust",
            Some("Parse input into an AST"),
        );
        let s = result.expect("signature present → Some");
        assert!(s.contains("pub fn parse"));
        assert!(s.contains("(rust)"));
        assert!(s.contains("Parse input into an AST"));
    }

    #[test]
    fn test_format_signature_none_returns_none() {
        let result = format_signature_for_embedding(None, "rust", Some("doc"));
        assert!(result.is_none());
    }

    #[test]
    fn test_format_signature_without_docstring() {
        let result = format_signature_for_embedding(Some("fn foo() -> bool"), "rust", None);
        let s = result.expect("signature present → Some");
        assert!(s.contains("fn foo() -> bool"));
        assert!(!s.contains("\n\n"));
    }

    // Integration test - only run if model download is acceptable
    #[test]
    #[ignore = "downloads model, run with --ignored"]
    fn test_embedder_produces_correct_dimensions() {
        use crate::seams::Embedder;
        let embedder = FastEmbedImpl::new().expect("failed to init embedder");
        let embedding = embedder.embed_one("test text").expect("failed to embed");

        assert_eq!(embedding.len(), 384);
        assert_eq!(embedder.dimension(), 384);
    }

    #[test]
    #[ignore = "downloads model, run with --ignored"]
    fn test_embed_batch() {
        use crate::seams::Embedder;
        let embedder = FastEmbedImpl::new().expect("failed to init embedder");
        let embeddings = embedder
            .embed_batch(&["first", "second", "third"])
            .expect("failed to embed");

        assert_eq!(embeddings.len(), 3);
        assert!(embeddings.iter().all(|e| e.len() == 384));
    }

    #[test]
    #[ignore = "downloads model, run with --ignored"]
    fn test_embed_empty_batch() {
        use crate::seams::Embedder;
        let embedder = FastEmbedImpl::new().expect("failed to init embedder");
        let embeddings = embedder.embed_batch(&[]).expect("failed to embed");

        assert!(embeddings.is_empty());
    }
}
