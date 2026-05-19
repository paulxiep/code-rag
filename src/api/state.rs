use std::sync::Arc;

use crate::engine::intent::IntentClassifier;
use crate::engine::{EngineConfig, LlmClient, RerankConfig, RigGeminiImpl};
use crate::store::{Embedder, FastEmbedImpl, MsMarcoRerankerImpl, Reranker, VectorReader};

/// Shared state for all handlers.
///
/// Caravan-RPC seam impls (`Embedder`, `Reranker`, `VectorReader`,
/// `LlmClient`) are registered with the SDK in [`AppState::from_config`] via
/// `caravan_rpc::provide`. Callers reach them through
/// `caravan_rpc::client::<dyn I>()`; AppState carries only the non-seam
/// state (the prototype classifier built once at startup, plus run config).
pub struct AppState {
    /// Pre-computed prototype embeddings for intent classification.
    pub classifier: IntentClassifier,
    pub config: EngineConfig,
}

impl AppState {
    pub async fn from_config(
        db_path: &str,
        model: &str,
        enable_reranker: bool,
    ) -> anyhow::Result<Arc<Self>> {
        // 1. Build seam impls.
        let embedder: Arc<dyn Embedder> = Arc::new(FastEmbedImpl::new()?);

        // Build the intent classifier with the embedder we just built — this
        // happens before `provide()` so we don't have to round-trip through the
        // registry for an initialization-only call.
        let classifier = IntentClassifier::build(|texts: &[&str]| embedder.embed_batch(texts))?;

        let store: Arc<dyn VectorReader> =
            Arc::new(crate::store::VectorStore::new(db_path, embedder.dimension()).await?);
        let llm: Arc<dyn LlmClient> = Arc::new(RigGeminiImpl::from_env(model)?);

        let mut config = EngineConfig::default();

        let reranker: Option<Arc<dyn Reranker>> = if enable_reranker {
            tracing::info!("initializing reranker (auto-downloading ms-marco-MiniLM-L-6-v2)");
            let r = MsMarcoRerankerImpl::new()?;
            config.rerank = RerankConfig {
                enabled: true,
                ..Default::default()
            };
            Some(Arc::new(r))
        } else {
            None
        };

        // 2. Register with the Caravan RPC SDK. Once registered,
        // `caravan_rpc::client::<dyn I>()` returns the same `Arc` we
        // constructed above (no overhead when `CARAVAN_RPC_PEERS` is unset).
        caravan_rpc::provide::<dyn Embedder>(embedder);
        caravan_rpc::provide::<dyn VectorReader>(store);
        caravan_rpc::provide::<dyn LlmClient>(llm);
        if let Some(r) = reranker {
            caravan_rpc::provide::<dyn Reranker>(r);
        }

        Ok(Arc::new(Self { classifier, config }))
    }
}
