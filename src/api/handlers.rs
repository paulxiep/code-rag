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

    // C3: hold the embedder lock through retrieve() — Comparison decomposition
    // needs to embed augmented per-comparator sub-queries inside the retriever.
    // Lock window grows from ~5ms to retrieve()-duration but the embedder is
    // process-local and contention is low.
    let mut embedder_guard = state.embedder.lock().await;
    let query_embedding = embedder_guard.embed_one(query)?;

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

    // Retrieve with optional reranking
    let mut reranker_guard = match &state.reranker {
        Some(r) => Some(r.lock().await),
        None => None,
    };
    let result = retriever::retrieve(
        retriever::QueryContext {
            query,
            embedding: &query_embedding,
            intent,
        },
        &state.store,
        &mut embedder_guard,
        &retrieval_config,
        &state.config,
        reranker_guard.as_deref_mut(),
    )
    .await?;
    drop(reranker_guard); // Release lock before LLM call
    drop(embedder_guard); // Release embedder lock before LLM call

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
