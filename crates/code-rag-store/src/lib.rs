//! coderag-store: Shared vector storage and embedding for code RAG
//!
//! This crate provides LanceDB-backed vector storage and embedding utilities
//! shared between code-raptor (writes) and code-rag-chat (reads).

pub mod embedder;
pub mod reranker;
pub mod seams;
pub mod vector_store;

pub use embedder::{
    EmbedError, FastEmbedImpl, format_code_for_embedding, format_crate_for_embedding,
    format_module_doc_for_embedding, format_readme_for_embedding, format_signature_for_embedding,
};
pub use reranker::{MsMarcoRerankerImpl, RerankError};
pub use seams::LlmError;
pub use vector_store::{StoreError, VectorStore};
// Seam traits live in the `seams` module. Direct re-exports at the crate root
// are deferred until B3 renames the concrete structs (avoids name collisions
// between e.g. `embedder::Embedder` (struct) and `seams::Embedder` (trait)).
