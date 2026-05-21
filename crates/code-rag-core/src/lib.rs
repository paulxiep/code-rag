//! `code-rag-core` — chat-side core shared between the `code-rag-chat`
//! root binary and the `code-rag-mcp` MCP server.
//!
//! Extracted at M5 to break MCP's transitive dependency on
//! `code-rag-chat` (the binary crate, unbuildable as a library by other
//! crates). HTTP routing, askama templates, and binary main stay in
//! the chat root.

pub mod dto;
pub mod errors;
pub mod retriever;
pub mod state;

pub use dto::{SourceInfo, build_sources};
pub use errors::EngineError;
pub use retriever::{QueryContext, RetrievalResult, retrieve, to_retrieval_result};
pub use state::AppState;

// Re-export the seam types so callers (mcp, chat) can write
// `code_rag_core::LlmClient` without also depending on code-rag-store
// directly. Callers that need richer access can still import
// `code_rag_store` directly.
pub use code_rag_store::seams::{Embedder, LlmClient, Reranker, VectorReader, VectorWriter};
