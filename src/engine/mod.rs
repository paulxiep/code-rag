mod config;
pub mod context;
pub mod generator;
pub mod intent;
pub mod retriever;

pub use config::EngineConfig;
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
}
