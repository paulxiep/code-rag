use axum::{Json, extract::State};
use std::sync::Arc;

use super::dto::{self, *};
use super::error::ApiError;
use super::state::AppState;
use crate::engine::{LlmClient, context, intent, retriever};
use crate::store::{Embedder, Reranker, VectorReader};

/// POST /chat - Ask a question about the portfolio
pub async fn chat(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ChatRequest>,
) -> Result<Json<ChatResponse>, ApiError> {
    let query = req.query.trim();
    if query.is_empty() {
        return Err(ApiError::BadRequest("Query cannot be empty".into()));
    }

    // Pull seam handles from the Caravan RPC registry. When
    // `CARAVAN_RPC_PEERS` is unset (normal local-run / compose), each
    // `client::<dyn I>()` is a registry lookup that returns the `Arc`
    // registered at AppState::from_config — no overhead.
    let embedder = caravan_rpc::client::<dyn Embedder>();
    let store = caravan_rpc::client::<dyn VectorReader>();
    let llm = caravan_rpc::client::<dyn LlmClient>();
    let reranker = caravan_rpc::try_client::<dyn Reranker>();

    let query_embedding = embedder.embed_one(query)?;

    // Keyword pre-filter for unambiguous comparison cues, else embedding classification.
    let intent = if let Some(pre) = intent::pre_classify_comparison(query) {
        tracing::info!(intent = ?pre, "query classified via keyword pre-filter");
        pre
    } else {
        let classification = intent::classify(&query_embedding, &state.classifier);
        tracing::info!(intent = ?classification.intent, confidence = classification.confidence, "query classified");
        classification.intent
    };
    let retrieval_config = intent::route(intent, &state.config.routing);

    let result = retriever::retrieve(
        retriever::QueryContext {
            query,
            embedding: &query_embedding,
            intent,
        },
        store.as_ref(),
        embedder.as_ref(),
        &retrieval_config,
        &state.config,
        reranker.as_deref(),
    )
    .await?;

    // Build context (pure function)
    let context = context::build_context(&result);
    let prompt = context::build_prompt(query, &context);

    // LLM call runs without any lock (slow: 2-5 seconds)
    let answer = llm.generate(&prompt).await?;

    // Build response
    let sources = dto::build_sources(&result);
    let intent = result.intent;

    Ok(Json(ChatResponse {
        answer,
        sources,
        intent,
    }))
}

pub async fn list_projects(
    State(_state): State<Arc<AppState>>,
) -> Result<Json<ProjectsResponse>, ApiError> {
    let store = caravan_rpc::client::<dyn VectorReader>();
    let projects = store.list_projects().await?;
    let count = projects.len();

    Ok(Json(ProjectsResponse { projects, count }))
}

/// GET /health - Health check
pub async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
    })
}
