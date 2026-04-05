pub mod generator;
pub mod retriever;

// Re-export shared engine logic from the platform-agnostic crate
pub use code_rag_engine::context;
pub use code_rag_engine::intent;

pub use code_rag_engine::config::RetrievalConfig;
pub use code_rag_engine::config::{DualEmbeddingConfig, EngineConfig, HybridConfig, RerankConfig};
pub use code_rag_engine::retriever::FlatChunk;
pub use generator::LlmClient;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum EngineError {
    #[error("embedding failed: {0}")]
    Embedding(#[from] code_rag_store::EmbedError),

    #[error("store error: {0}")]
    Store(#[from] code_rag_store::StoreError),

    #[error("generation failed: {0}")]
    Generation(String),

    #[error("reranking failed: {0}")]
    Rerank(String),
}
