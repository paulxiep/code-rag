use axum::{Json, extract::State};
use std::sync::Arc;

use super::dto::{self, *};
use super::error::ApiError;
use super::state::AppState;
use crate::engine::{context, generator, intent, retriever};

/// POST /chat - Ask a question about the portfolio
pub async fn chat(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ChatRequest>,
) -> Result<Json<ChatResponse>, ApiError> {
    let query = req.query.trim();
    if query.is_empty() {
        return Err(ApiError::BadRequest("Query cannot be empty".into()));
    }

    // Embed query once (lock held ~5ms only)
    let query_embedding = {
        let mut embedder = state.embedder.lock().await;
        embedder.embed_one(query)?
    };

    // Classify using prototype similarity (no lock needed)
    let classification = intent::classify(&query_embedding, &state.classifier);
    let retrieval_config = intent::route(classification.intent, &state.config.routing);
    tracing::info!(intent = ?classification.intent, confidence = classification.confidence, "query classified");

    // Retrieve with pre-computed embedding and intent
    let result = retriever::retrieve(
        &query_embedding,
        &state.store,
        &retrieval_config,
        classification.intent,
    )
    .await?;

    // Build context (pure function)
    let context = context::build_context(&result);
    let prompt = context::build_prompt(query, &context);

    // LLM call runs without any lock (slow: 2-5 seconds)
    let answer = generator::generate(&prompt, &state.llm).await?;

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
    State(state): State<Arc<AppState>>,
) -> Result<Json<ProjectsResponse>, ApiError> {
    let projects = state.store.list_projects().await?;
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
