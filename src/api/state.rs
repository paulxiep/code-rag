use std::sync::Arc;
use tokio::sync::Mutex;

use crate::engine::intent::IntentClassifier;
use crate::engine::{EngineConfig, LlmClient};
use crate::store::{Embedder, VectorStore};

/// Shared state for all handlers
pub struct AppState {
    // Only embedder needs mutation
    pub embedder: Mutex<Embedder>,

    // Pre-computed prototype embeddings for intent classification
    pub classifier: IntentClassifier,

    // These are safe to share (internal connection pooling)
    pub store: VectorStore,
    pub llm: LlmClient,
    pub config: EngineConfig,
}

impl AppState {
    pub async fn from_config(db_path: &str, model: &str) -> anyhow::Result<Arc<Self>> {
        let mut embedder = Embedder::new()?;

        // Build classifier BEFORE wrapping embedder in Mutex
        let classifier = IntentClassifier::build(&mut embedder)?;

        let store = VectorStore::new(db_path, embedder.dimension()).await?;
        let llm = LlmClient::from_env(model)?;
        let config = EngineConfig::default();

        Ok(Arc::new(Self {
            embedder: Mutex::new(embedder),
            classifier,
            store,
            llm,
            config,
        }))
    }
}
