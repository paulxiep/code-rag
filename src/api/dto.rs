use serde::{Deserialize, Serialize};

use code_rag_engine::intent::QueryIntent;

// SourceInfo + build_sources moved to `code-rag-core` at M5 so MCP
// (and any future consumer) can format chat responses without pulling
// in the chat binary. Re-exported here for back-compat with existing
// call sites that use `crate::api::{SourceInfo, build_sources}` (or
// `code_rag_chat::api::*`).
pub use code_rag_core::{SourceInfo, build_sources};

/// POST /chat request
#[derive(Debug, Deserialize)]
pub struct ChatRequest {
    pub query: String,
}

/// POST /chat response
#[derive(Debug, Serialize)]
pub struct ChatResponse {
    pub answer: String,
    pub sources: Vec<SourceInfo>,
    pub intent: QueryIntent,
}

/// GET /health response
#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub version: &'static str,
}

/// GET /projects response
#[derive(Debug, Serialize)]
pub struct ProjectsResponse {
    pub projects: Vec<String>,
    pub count: usize,
}
