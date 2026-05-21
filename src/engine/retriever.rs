// `retrieve()` orchestrator moved to `code-rag-core::retriever` at M5
// so MCP can call it without depending on the chat binary. Re-exported
// here for back-compat with existing call sites like
// `crate::engine::retriever::retrieve(...)`.
pub use code_rag_core::retriever::{QueryContext, RetrievalResult, retrieve, to_retrieval_result};
