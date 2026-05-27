//! Local impl of the [`IntentClassifier`] seam (M7).
//!
//! Wraps the pure-math classifier from `code-rag-engine` so the chat path
//! can dispatch through `caravan_rpc::client::<dyn IntentClassifier>()`.
//! In `mode: inproc` (default for compose targets) this is a direct call
//! into the wrapped struct; in `mode: lambda` (`prod-mixed`) the same
//! interface dispatches over SigV4 to a Lambda Function URL.
//!
//! The browser/WASM bundle (`code-rag-ui::standalone_api`) does not go
//! through this seam — it constructs the engine struct directly because
//! it can't host a Caravan SDK registry.

use code_rag_engine::intent::{self, ClassificationResult, IntentClassifier as EngineClassifier};
use code_rag_store::seams::{IntentClassifier, IntentError};

/// Holds an `EngineClassifier` (the prototype-embedding-bearing struct)
/// and delegates `classify` to the engine's `intent::classify` free
/// function. Built once at startup alongside the other seam impls.
pub struct LocalIntentClassifier {
    inner: EngineClassifier,
}

impl LocalIntentClassifier {
    pub fn new(inner: EngineClassifier) -> Self {
        Self { inner }
    }
}

impl IntentClassifier for LocalIntentClassifier {
    fn classify(&self, query_embedding: &[f32]) -> Result<ClassificationResult, IntentError> {
        // intent::classify is infallible at the engine layer — wrap into
        // Result<_, IntentError> for the wire shape.
        Ok(intent::classify(query_embedding, &self.inner))
    }
}
