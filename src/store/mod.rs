//! Re-export storage functionality from shared crate.
//!
//! The seam traits (`Embedder`, `Reranker`, `VectorReader`, ...) from
//! `code_rag_store::seams` are the caravan-rpc dispatch boundary; the
//! concrete `FastEmbedImpl` / `MsMarcoRerankerImpl` types are what
//! AppState constructs and `provide()`s.

pub use code_rag_store::seams::{Embedder, Reranker, VectorReader};
pub use code_rag_store::{
    FastEmbedImpl, MsMarcoRerankerImpl, RerankError, StoreError, VectorStore,
};
