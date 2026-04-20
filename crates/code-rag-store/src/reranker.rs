use fastembed::{
    OnnxSource, RerankInitOptionsUserDefined, RerankResult, TextRerank, TokenizerFiles,
    UserDefinedRerankingModel,
};
use hf_hub::api::sync::Api;
use std::path::Path;
use thiserror::Error;

const HF_MODEL_ID: &str = "cross-encoder/ms-marco-MiniLM-L-6-v2";
const ONNX_FILE: &str = "onnx/model.onnx";
const TOKENIZER_FILE: &str = "tokenizer.json";
const CONFIG_FILE: &str = "config.json";
const SPECIAL_TOKENS_FILE: &str = "special_tokens_map.json";
const TOKENIZER_CONFIG_FILE: &str = "tokenizer_config.json";

#[derive(Error, Debug)]
pub enum RerankError {
    #[error("failed to initialize reranker model: {0}")]
    Init(#[from] anyhow::Error),

    #[error("reranking failed: {0}")]
    Rerank(String),

    #[error("model files not found at {0}")]
    ModelNotFound(String),
}

/// Cross-encoder reranker. Wraps fastembed's TextRerank.
/// Holds loaded ONNX model weights in memory.
/// Uses ms-marco-MiniLM-L-6-v2 — same model as browser (Xenova/ms-marco-MiniLM-L-6-v2).
pub struct Reranker {
    model: TextRerank,
}

impl Reranker {
    /// Auto-download model from HuggingFace Hub and initialize.
    /// Files are cached locally (same cache mechanism as fastembed embedder).
    ///
    /// If the `CODE_RAG_RERANKER_DIR` env var is set, loads model files from
    /// that directory instead of fetching from HF. This is the hook bundled
    /// releases use to avoid network access on first run — point it at a
    /// directory containing the ms-marco-MiniLM-L-6-v2 files (`model.onnx` or
    /// `onnx/model.onnx`, plus `tokenizer.json`, `config.json`,
    /// `special_tokens_map.json`, and optionally `tokenizer_config.json`).
    pub fn new() -> Result<Self, RerankError> {
        if let Some(dir) = std::env::var_os("CODE_RAG_RERANKER_DIR") {
            let path = std::path::PathBuf::from(dir);
            return Self::from_dir(&path);
        }

        let api = Api::new().map_err(|e| RerankError::Init(e.into()))?;
        let repo = api.model(HF_MODEL_ID.to_string());

        // Download all required files (cached after first download)
        let onnx_path = repo
            .get(ONNX_FILE)
            .map_err(|e| RerankError::Init(e.into()))?;
        let tokenizer_bytes = std::fs::read(
            repo.get(TOKENIZER_FILE)
                .map_err(|e| RerankError::Init(e.into()))?,
        )
        .map_err(|e| RerankError::Init(e.into()))?;
        let config_bytes = std::fs::read(
            repo.get(CONFIG_FILE)
                .map_err(|e| RerankError::Init(e.into()))?,
        )
        .map_err(|e| RerankError::Init(e.into()))?;
        let special_tokens_bytes = std::fs::read(
            repo.get(SPECIAL_TOKENS_FILE)
                .map_err(|e| RerankError::Init(e.into()))?,
        )
        .map_err(|e| RerankError::Init(e.into()))?;
        let tokenizer_config_bytes = std::fs::read(
            repo.get(TOKENIZER_CONFIG_FILE)
                .map_err(|e| RerankError::Init(e.into()))?,
        )
        .unwrap_or_default();

        Self::from_files(
            &onnx_path,
            tokenizer_bytes,
            config_bytes,
            special_tokens_bytes,
            tokenizer_config_bytes,
        )
    }

    /// Initialize from a local directory containing pre-downloaded model files.
    pub fn from_dir(model_dir: &Path) -> Result<Self, RerankError> {
        // Support both flat layout and onnx/ subdirectory layout
        let onnx_path = if model_dir.join(ONNX_FILE).exists() {
            model_dir.join(ONNX_FILE)
        } else {
            model_dir.join("model.onnx")
        };

        if !onnx_path.exists() {
            return Err(RerankError::ModelNotFound(model_dir.display().to_string()));
        }

        let tokenizer_bytes = std::fs::read(model_dir.join(TOKENIZER_FILE))
            .map_err(|e| RerankError::Init(e.into()))?;
        let config_bytes =
            std::fs::read(model_dir.join(CONFIG_FILE)).map_err(|e| RerankError::Init(e.into()))?;
        let special_tokens_bytes = std::fs::read(model_dir.join(SPECIAL_TOKENS_FILE))
            .map_err(|e| RerankError::Init(e.into()))?;
        let tokenizer_config_bytes =
            std::fs::read(model_dir.join(TOKENIZER_CONFIG_FILE)).unwrap_or_default();

        Self::from_files(
            &onnx_path,
            tokenizer_bytes,
            config_bytes,
            special_tokens_bytes,
            tokenizer_config_bytes,
        )
    }

    fn from_files(
        onnx_path: &Path,
        tokenizer_bytes: Vec<u8>,
        config_bytes: Vec<u8>,
        special_tokens_bytes: Vec<u8>,
        tokenizer_config_bytes: Vec<u8>,
    ) -> Result<Self, RerankError> {
        let user_model = UserDefinedRerankingModel::new(
            OnnxSource::File(onnx_path.to_path_buf()),
            TokenizerFiles {
                tokenizer_file: tokenizer_bytes,
                config_file: config_bytes,
                special_tokens_map_file: special_tokens_bytes,
                tokenizer_config_file: tokenizer_config_bytes,
            },
        );

        let model = TextRerank::try_new_from_user_defined(
            user_model,
            RerankInitOptionsUserDefined::default(),
        )?;

        Ok(Self { model })
    }

    /// Rerank documents against a query.
    /// Returns Vec<RerankResult> sorted by score descending.
    pub fn rerank(
        &mut self,
        query: &str,
        documents: Vec<String>,
    ) -> Result<Vec<RerankResult>, RerankError> {
        if documents.is_empty() {
            return Ok(Vec::new());
        }

        self.model
            .rerank(query.to_string(), &documents, false, None)
            .map_err(|e| RerankError::Rerank(e.to_string()))
    }
}
