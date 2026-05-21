use thiserror::Error;

/// Errors flowing out of the chat-side retrieval orchestrator. Wraps
/// the typed errors from the store and embedder seams plus the LLM
/// seam's `LlmError` and a free-form "rerank failed" arm.
///
/// Lived in `code-rag-chat::engine::mod` pre-M5; moved here so MCP
/// (and any future consumer) can map retrieval errors uniformly
/// without depending on the chat binary.
#[derive(Error, Debug)]
pub enum EngineError {
    #[error("embedding failed: {0}")]
    Embedding(#[from] code_rag_store::EmbedError),

    #[error("store error: {0}")]
    Store(#[from] code_rag_store::StoreError),

    #[error("generation failed: {0}")]
    Generation(String),

    #[error("LLM seam error: {0}")]
    Llm(#[from] code_rag_store::LlmError),

    #[error("reranking failed: {0}")]
    Rerank(String),
}
