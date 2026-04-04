//! coderag-store: Shared vector storage and embedding for code RAG
//!
//! This crate provides LanceDB-backed vector storage and embedding utilities
//! shared between code-raptor (writes) and code-rag-chat (reads).

pub mod embedder;
pub mod reranker;
pub mod vector_store;

pub use embedder::{
    EmbedError, Embedder, format_code_for_embedding, format_crate_for_embedding,
    format_module_doc_for_embedding, format_readme_for_embedding,
};
pub use reranker::{RerankError, Reranker};
pub use vector_store::{StoreError, VectorStore};
