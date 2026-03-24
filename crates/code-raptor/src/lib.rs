//! code-raptor: Code knowledge graph construction
//!
//! This crate handles parsing code repositories into chunks.
//! For embedding and storage, it uses the shared coderag-store crate.

pub mod ingestion;

pub use code_rag_types::{CodeChunk, CrateChunk, ModuleDocChunk, ReadmeChunk};
pub use ingestion::{
    DEFAULT_EMBEDDING_MODEL, DeletionsByTable, ExistingFileIndex, IngestionResult, IngestionStats,
    ReconcileResult, reconcile, run_ingestion,
};

// Re-export store functionality for convenience
pub use code_rag_store::{
    Embedder, StoreError, VectorStore, format_code_for_embedding, format_crate_for_embedding,
    format_module_doc_for_embedding, format_readme_for_embedding,
};
