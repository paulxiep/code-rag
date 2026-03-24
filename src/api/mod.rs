mod dto;
mod error;
mod handlers;
mod state;
mod web;

pub use state::AppState;

use axum::{
    Router,
    routing::{get, post},
};
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;

/// Build the application router
pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        // HTML routes (frontend)
        .route("/", get(web::index))
        .route("/api/chat", post(web::chat_html))
        .route("/api/projects-list", get(web::projects_list_html))
        // JSON API routes
        .route("/chat", post(handlers::chat))
        .route("/projects", get(handlers::list_projects))
        .route("/health", get(handlers::health))
        // Static files
        .nest_service("/static", ServeDir::new("static"))
        .layer(CorsLayer::permissive())
        .with_state(state)
}
