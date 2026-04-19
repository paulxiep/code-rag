//! code-rag MCP server — exposes the retrieval brain as Claude-Code tools.
//!
//! Phase 1 spike: one tool (`code_rag_search`). Phase 2 adds `code_rag_graph`,
//! `code_rag_overview`, `code_rag_neighbors`, and `code_rag_reindex`.
//!
//! Process model: single-request-at-a-time. The embedder and reranker ONNX
//! sessions are already `Mutex`-guarded inside `AppState`; tool handlers
//! acquire those locks briefly and drop them before returning. That matches
//! the HTTP server's behaviour at `src/api/handlers.rs:42`.
//!
//! Protocol channel: stdout. All tracing output MUST go to stderr, or the
//! JSON-RPC stream corrupts and the Claude Code client disconnects.

use std::sync::Arc;

use anyhow::Result;
use clap::Parser;
use code_rag_chat::{
    api::{AppState, build_sources},
    engine::{intent, retriever},
};
use rmcp::{
    ErrorData as McpError, ServerHandler, ServiceExt,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, Content, Implementation, ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router,
    transport::stdio,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tracing_subscriber::EnvFilter;

/// CLI for the MCP stdio binary. External users point `--db-path` at an
/// `index.lance` produced by `code-raptor ingest . --single-repo --full`.
#[derive(Parser, Debug)]
#[command(name = "code-rag-mcp", about = "MCP server for code-rag retrieval")]
struct Cli {
    /// Path to the LanceDB index produced by `code-raptor ingest ... --single-repo`.
    #[arg(long, default_value = "./.code-rag/index.lance")]
    db_path: String,

    /// LLM model name — AppState requires one even though MCP never calls the
    /// LLM. Value is unused at tool-call time; any string is fine.
    #[arg(long, default_value = "gemini-2.5-flash")]
    model: String,

    /// Disable the cross-encoder reranker. Useful when running on machines
    /// without the ONNX model cached and offline.
    #[arg(long)]
    no_rerank: bool,
}

/// Intent hint for `code_rag_search`. When absent, the classifier decides.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
enum IntentHint {
    Overview,
    Implementation,
    Relationship,
    Comparison,
}

impl From<IntentHint> for code_rag_engine::intent::QueryIntent {
    fn from(h: IntentHint) -> Self {
        use code_rag_engine::intent::QueryIntent;
        match h {
            IntentHint::Overview => QueryIntent::Overview,
            IntentHint::Implementation => QueryIntent::Implementation,
            IntentHint::Relationship => QueryIntent::Relationship,
            IntentHint::Comparison => QueryIntent::Comparison,
        }
    }
}

/// Parameters for `code_rag_search`.
#[derive(Debug, Deserialize, JsonSchema)]
struct SearchParams {
    /// Natural-language query to retrieve code/doc chunks for.
    query: String,
    /// Optional intent override. Skip classification when Claude has a strong
    /// prior (e.g. it knows the question is architectural/"overview").
    #[serde(default)]
    intent: Option<IntentHint>,
}

/// Serialized result shape returned to the MCP client. Subset of `SourceInfo`
/// with the fields Claude Code actually needs to act on a hit.
#[derive(Debug, Serialize)]
struct SearchHit {
    #[serde(rename = "type")]
    chunk_type: String,
    chunk_id: String,
    path: String,
    label: String,
    project: String,
    relevance: f32,
    line: usize,
}

#[derive(Clone)]
struct CodeRagServer {
    state: Arc<AppState>,
    // Read by rmcp's tool_handler via trait dispatch; dead_code analysis misses it.
    #[allow(dead_code)]
    tool_router: ToolRouter<CodeRagServer>,
}

#[tool_router]
impl CodeRagServer {
    fn new(state: Arc<AppState>) -> Self {
        Self {
            state,
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        description = "Semantic + hybrid search over the indexed repository. Routes by query intent (overview / implementation / relationship / comparison) and returns ranked code, README, folder, and module-doc chunks. Pass `intent` to override the classifier."
    )]
    async fn code_rag_search(
        &self,
        Parameters(params): Parameters<SearchParams>,
    ) -> Result<CallToolResult, McpError> {
        let SearchParams { query, intent: intent_override } = params;
        let query = query.trim();
        if query.is_empty() {
            return Err(McpError::invalid_params("query must not be empty", None));
        }

        // Embed the query. Lock held through retrieve() because Comparison
        // decomposition embeds augmented sub-queries inline.
        let mut embedder_guard = self.state.embedder.lock().await;
        let query_embedding = embedder_guard
            .embed_one(query)
            .map_err(|e| McpError::internal_error(format!("embed failed: {e}"), None))?;

        // Intent: override > keyword pre-classify > classifier.
        let query_intent = if let Some(hint) = intent_override {
            hint.into()
        } else if let Some(pre) = intent::pre_classify_comparison(query) {
            pre
        } else {
            intent::classify(&query_embedding, &self.state.classifier).intent
        };
        let retrieval_config = intent::route(query_intent, &self.state.config.routing);

        let mut reranker_guard = match &self.state.reranker {
            Some(r) => Some(r.lock().await),
            None => None,
        };
        let result = retriever::retrieve(
            retriever::QueryContext {
                query,
                embedding: &query_embedding,
                intent: query_intent,
            },
            &self.state.store,
            &mut embedder_guard,
            &retrieval_config,
            &self.state.config,
            reranker_guard.as_deref_mut(),
        )
        .await
        .map_err(|e| McpError::internal_error(format!("retrieve failed: {e}"), None))?;
        drop(reranker_guard);
        drop(embedder_guard);

        let hits: Vec<SearchHit> = build_sources(&result)
            .into_iter()
            .map(|s| SearchHit {
                chunk_type: s.chunk_type,
                chunk_id: s.chunk_id,
                path: s.path,
                label: s.label,
                project: s.project,
                relevance: s.relevance,
                line: s.line,
            })
            .collect();

        let body = serde_json::to_string_pretty(&serde_json::json!({
            "intent": format!("{:?}", result.intent),
            "hits": hits,
        }))
        .map_err(|e| McpError::internal_error(format!("serialize failed: {e}"), None))?;

        Ok(CallToolResult::success(vec![Content::text(body)]))
    }
}

#[tool_handler]
impl ServerHandler for CodeRagServer {
    fn get_info(&self) -> ServerInfo {
        // `Implementation::from_build_env()` reads rmcp's own CARGO_PKG vars
        // (since it's defined inside the rmcp crate), so we construct ours
        // explicitly from this binary's compile-time env.
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(
                env!("CARGO_PKG_NAME"),
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(
                "Intent-routed semantic retrieval over an indexed repo. \
                 Prefer Grep/Read for exact identifiers and just-edited code; \
                 reach for code_rag_search on conceptual queries.",
            )
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // stdout is the MCP protocol channel — route all log output to stderr.
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,lance::file_audit=warn")),
        )
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    let cli = Cli::parse();
    tracing::info!(db_path = %cli.db_path, "starting code-rag MCP server");

    // AppState::from_config wires up an LlmClient whose constructor panics if
    // GEMINI_API_KEY is unset. The MCP surface never calls the LLM (Claude
    // Code is the LLM — we're only the retriever), so inject a placeholder if
    // the user hasn't set one. External users shouldn't need an API key to
    // run retrieval. SAFETY: set_var is safe here because it runs before any
    // other thread is spawned by tokio::main's runtime.
    if std::env::var_os("GEMINI_API_KEY").is_none() {
        // SAFETY: single-threaded at this point; no other reader of the env.
        unsafe {
            std::env::set_var(
                "GEMINI_API_KEY",
                "unused-mcp-does-not-call-llm",
            );
        }
    }

    let state = AppState::from_config(&cli.db_path, &cli.model, !cli.no_rerank)
        .await
        .map_err(|e| anyhow::anyhow!("AppState init failed: {e}"))?;

    let server = CodeRagServer::new(state);
    let service = server.serve(stdio()).await.inspect_err(|e| {
        tracing::error!("serve error: {e:?}");
    })?;

    service.waiting().await?;
    Ok(())
}
