pub mod retriever;

// Re-export shared engine logic from the platform-agnostic crate
pub use code_rag_engine::context;
pub use code_rag_engine::intent;

pub use code_rag_engine::config::RetrievalConfig;
pub use code_rag_engine::config::{DualEmbeddingConfig, EngineConfig, HybridConfig, RerankConfig};
pub use code_rag_engine::retriever::FlatChunk;
pub use code_rag_store::seams::LlmClient;

// M5: `RigGeminiImpl` moved to the dedicated `code-rag-llm` crate so a
// Caravan-emitted synthetic peer can load the impl from a library crate
// (peers can't pull in `code-rag-chat`, the host binary). Re-exported
// here for back-compat with existing call sites like
// `crate::engine::RigGeminiImpl` and `code_rag_chat::engine::RigGeminiImpl`.
pub use code_rag_llm::RigGeminiImpl;

// M5: `EngineError` moved to `code-rag-core::errors` alongside the
// extracted `retrieve()` orchestrator. Re-exported here so existing
// call sites in handlers / bin / harness keep compiling.
pub use code_rag_core::EngineError;
