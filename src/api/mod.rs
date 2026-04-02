mod dto;
mod error;
mod handlers;
mod state;

pub use state::AppState;

use axum::{
    Router,
    routing::{get, post},
};
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};

/// Build the application router
pub fn router(state: Arc<AppState>) -> Router {
    // Path to the WASM UI dist directory
    let ui_dist = std::env::var("UI_DIST").unwrap_or_else(|_| "crates/code-rag-ui/dist".into());

    let index_file = format!("{}/index.html", ui_dist);

    Router::new()
        // JSON API routes
        .route("/chat", post(handlers::chat))
        .route("/projects", get(handlers::list_projects))
        .route("/health", get(handlers::health))
        // Serve WASM UI — static files + SPA fallback to index.html
        .fallback_service(ServeDir::new(&ui_dist).not_found_service(ServeFile::new(&index_file)))
        .layer(CorsLayer::permissive())
        .with_state(state)
}
