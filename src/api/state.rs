// AppState moved to `code-rag-core` at M5 so MCP can depend on it
// without pulling in the chat binary. Re-exported here for back-compat
// with existing call sites that use `crate::api::AppState` (or
// `code_rag_chat::api::AppState`).
pub use code_rag_core::AppState;
