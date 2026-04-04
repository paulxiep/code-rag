use std::sync::Arc;
use tokio::sync::Mutex;

use crate::engine::intent::IntentClassifier;
use crate::engine::{EngineConfig, LlmClient, RerankConfig};
use crate::store::{Embedder, Reranker, VectorStore};

/// Shared state for all handlers
pub struct AppState {
    // Only embedder and reranker need mutation (fastembed requires &mut self)
    pub embedder: Mutex<Embedder>,
    pub reranker: Option<Mutex<Reranker>>,

    // Pre-computed prototype embeddings for intent classification
    pub classifier: IntentClassifier,

    // These are safe to share (internal connection pooling)
    pub store: VectorStore,
    pub llm: LlmClient,
    pub config: EngineConfig,
}

impl AppState {
    pub async fn from_config(
        db_path: &str,
        model: &str,
        enable_reranker: bool,
    ) -> anyhow::Result<Arc<Self>> {
        let mut embedder = Embedder::new()?;

        // Build classifier BEFORE wrapping embedder in Mutex
        let classifier = IntentClassifier::build(|texts: &[&str]| embedder.embed_batch(texts))?;

        let store = VectorStore::new(db_path, embedder.dimension()).await?;
        let llm = LlmClient::from_env(model)?;

        let mut config = EngineConfig::default();

        let reranker = if enable_reranker {
            tracing::info!("initializing reranker (auto-downloading ms-marco-MiniLM-L-6-v2)");
            let r = Reranker::new()?;
            config.rerank = RerankConfig {
                enabled: true,
                ..Default::default()
            };
            Some(Mutex::new(r))
        } else {
            None
        };

        Ok(Arc::new(Self {
            embedder: Mutex::new(embedder),
            reranker,
            classifier,
            store,
            llm,
            config,
        }))
    }
}
