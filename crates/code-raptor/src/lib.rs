//! code-raptor: Code knowledge graph construction
//!
//! This crate handles parsing code repositories into chunks.
//! For embedding and storage, it uses the shared coderag-store crate.

pub mod edge_resolution;
pub mod ingestion;
pub mod orchestrate;

pub use code_rag_types::{CodeChunk, CrateChunk, ModuleDocChunk, ReadmeChunk};
pub use ingestion::{
    CallsMap, DEFAULT_EMBEDDING_MODEL, DeletionsByTable, ExistingFileIndex, ImportsMap,
    IngestionResult, IngestionStats, ReconcileResult, reconcile, run_ingestion,
};
pub use orchestrate::{IngestOpts, ingest_repo};

// Re-export store functionality for convenience
pub use code_rag_store::{
    Embedder, StoreError, VectorStore, format_code_for_embedding, format_crate_for_embedding,
    format_module_doc_for_embedding, format_readme_for_embedding, format_signature_for_embedding,
};
