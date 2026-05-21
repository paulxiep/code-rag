use code_rag_chat::api;
use code_rag_chat::store;

use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Warmup mode - just download model and exit
    if std::env::args().any(|a| a == "--warmup") {
        println!("Warming up embedding model...");
        let _ = crate::store::FastEmbedImpl::new();
        println!("Warmup complete");
        return Ok(());
    }

    // Health check mode
    if std::env::args().any(|a| a == "--health") {
        // Simple health check - just exit 0
        return Ok(());
    }

    // Initialize logging
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "code_rag_chat=info".into()),
        )
        .init();

    // Load environment variables
    dotenvy::dotenv().ok();

    // Configuration
    let db_path = std::env::var("DB_PATH").unwrap_or_else(|_| "./data/portfolio.lance".into());
    let model = std::env::var("GEMINI_MODEL").unwrap_or_else(|_| "gemini-3.1-flash-lite".into());
    let host = std::env::var("HOST").unwrap_or_else(|_| "127.0.0.1".into());
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3000);
    let enable_reranker = std::env::var("ENABLE_RERANKER")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);

    tracing::info!(db_path, model, enable_reranker, "Initializing application");

    // Build application state. `from_config` calls `caravan_rpc::provide()`
    // for each #[wagon] trait — must happen BEFORE `run_or_serve` so the
    // SDK has registered impls available in peer mode.
    let state = api::AppState::from_config(&db_path, &model, enable_reranker).await?;

    // Caravan-RPC contract: dispatches to peer-serve mode when
    // CARAVAN_RPC_ROLE=peer-<Interface> is set, otherwise runs the chat
    // server. Same binary serves both roles; the SDK picks via env var.
    caravan_rpc::run_or_serve(|| async move {
        let app = api::router(state);
        let addr = format!("{}:{}", host, port);
        tracing::info!("Starting server on http://{}", addr);
        let listener = tokio::net::TcpListener::bind(&addr).await.map_err(|e| {
            caravan_rpc::RpcError::Transport(caravan_rpc::RpcTransportError::Http(e.to_string()))
        })?;
        axum::serve(listener, app).await.map_err(|e| {
            caravan_rpc::RpcError::Transport(caravan_rpc::RpcTransportError::Http(e.to_string()))
        })?;
        Ok(())
    })
    .await?;

    Ok(())
}
