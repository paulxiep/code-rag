use askama::Template;
use axum::{
    Form,
    extract::State,
    http::StatusCode,
    response::{Html, IntoResponse, Response},
};
use std::sync::Arc;

use crate::api::dto::{self, SourceInfo};
use crate::api::state::AppState;
use crate::engine::{context, generator, intent, retriever};

/// Helper to render templates into axum responses
fn render_template<T: Template>(template: &T) -> Response {
    match template.render() {
        Ok(html) => Html(html).into_response(),
        Err(e) => {
            tracing::error!("Template render error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Template error: {}", e),
            )
                .into_response()
        }
    }
}

/// GET / - Main chat page
#[derive(Template)]
#[template(path = "chat.html")]
pub struct ChatPage;

pub async fn index() -> Response {
    render_template(&ChatPage)
}

/// POST /api/chat - Chat form submission (returns HTML fragment)
#[derive(serde::Deserialize)]
pub struct ChatForm {
    pub query: String,
}

#[derive(Template)]
#[template(path = "partials/message.html")]
pub struct MessageFragment {
    pub query: String,
    pub answer: String,
    pub sources: Vec<SourceInfo>,
    pub intent: String,
}

pub async fn chat_html(State(state): State<Arc<AppState>>, Form(form): Form<ChatForm>) -> Response {
    let query = form.query.trim().to_string();

    if query.is_empty() {
        return Html("<div class=\"message assistant\"><div class=\"message-content\"><strong>Assistant:</strong><p>Please enter a question.</p></div></div>".to_string()).into_response();
    }

    // Embed query once (lock held ~5ms only)
    let query_embedding = {
        let mut embedder = state.embedder.lock().await;
        match embedder.embed_one(&query) {
            Ok(emb) => emb,
            Err(e) => {
                tracing::error!("Embedding error: {}", e);
                return Html(format!(
                    "<div class=\"message user\"><div class=\"message-content\"><strong>You:</strong><p>{}</p></div></div>\
                     <div class=\"message assistant\"><div class=\"message-content\"><strong>Assistant:</strong><p>Sorry, I encountered an error processing your query.</p></div></div>",
                    query
                )).into_response();
            }
        }
    };

    // Classify using prototype similarity (no lock needed)
    let classification = intent::classify(&query_embedding, &state.classifier);
    let retrieval_config = intent::route(classification.intent, &state.config.routing);
    tracing::info!(intent = ?classification.intent, confidence = classification.confidence, "query classified");

    // Retrieve with pre-computed embedding and intent
    let result = match retriever::retrieve(
        &query_embedding,
        &state.store,
        &retrieval_config,
        classification.intent,
    )
    .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("Retrieval error: {}", e);
            return Html(format!(
                "<div class=\"message user\"><div class=\"message-content\"><strong>You:</strong><p>{}</p></div></div>\
                 <div class=\"message assistant\"><div class=\"message-content\"><strong>Assistant:</strong><p>Sorry, I encountered an error retrieving context.</p></div></div>",
                query
            )).into_response();
        }
    };

    // Build context and generate response
    let context = context::build_context(&result);
    let prompt = context::build_prompt(&query, &context);

    let answer = match generator::generate(&prompt, &state.llm).await {
        Ok(a) => a,
        Err(e) => {
            tracing::error!("Generation error: {}", e);
            format!("Sorry, I encountered an error: {}", e)
        }
    };

    let sources = dto::build_sources(&result);
    let intent = serde_json::to_value(result.intent)
        .ok()
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_else(|| "unknown".into());

    render_template(&MessageFragment {
        query,
        answer,
        sources,
        intent,
    })
}

/// GET /api/projects-list - Returns HTML fragment of projects
#[derive(Template)]
#[template(path = "partials/projects_list.html")]
pub struct ProjectsListFragment {
    pub projects: Vec<String>,
    pub count: usize,
}

pub async fn projects_list_html(State(state): State<Arc<AppState>>) -> Response {
    let projects = state.store.list_projects().await.unwrap_or_default();
    let count = projects.len();

    render_template(&ProjectsListFragment { projects, count })
}
