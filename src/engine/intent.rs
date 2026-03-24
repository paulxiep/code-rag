use serde::Serialize;
use std::collections::HashMap;

use super::EngineError;
use super::config::RetrievalConfig;
use crate::store::Embedder;

/// Query intent categories.
///
/// Extensible: new variants added for Track A (Hierarchy), Track B (Identifier).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryIntent {
    /// "What does X do?", "Tell me about Y", "Overview of Z"
    Overview,
    /// "How does X work?", "Show me the implementation of Y"
    Implementation,
    /// "What calls X?", "How does A relate to B?"
    Relationship,
    /// "How does A compare to B?", "Differences between X and Y"
    Comparison,
}

// --- Prototype queries (static data, replaces keyword lists) ---

const OVERVIEW_PROTOTYPES: &[&str] = &[
    "What is this project?",
    "Tell me about this codebase",
    "Give me an overview",
    "What does this do?",
    "Describe the purpose",
    "What is the architecture?",
];

const IMPLEMENTATION_PROTOTYPES: &[&str] = &[
    "How does this function work?",
    "Show me the implementation",
    "How is this implemented?",
    "What does this code do?",
    "Walk me through the logic",
];

const RELATIONSHIP_PROTOTYPES: &[&str] = &[
    "What calls this function?",
    "How does A relate to B?",
    "What depends on this?",
    "Show me the call chain",
    "What uses this module?",
];

const COMPARISON_PROTOTYPES: &[&str] = &[
    "Compare A and B",
    "What are the differences between X and Y?",
    "How does A differ from B?",
    "A versus B",
    "Contrast these approaches",
    "What are the pros and cons?",
];

/// Pre-computed prototype embeddings for each intent.
/// Built once at startup; used for every classification call.
pub struct IntentClassifier {
    prototypes: HashMap<QueryIntent, Vec<Vec<f32>>>,
    default: QueryIntent,
    threshold: f32,
}

impl IntentClassifier {
    /// Build the classifier by embedding all prototype queries.
    /// Called once at startup before embedder is wrapped in Mutex.
    pub fn build(embedder: &mut Embedder) -> Result<Self, EngineError> {
        let mut prototypes = HashMap::new();

        for (intent, texts) in [
            (QueryIntent::Overview, OVERVIEW_PROTOTYPES),
            (QueryIntent::Implementation, IMPLEMENTATION_PROTOTYPES),
            (QueryIntent::Relationship, RELATIONSHIP_PROTOTYPES),
            (QueryIntent::Comparison, COMPARISON_PROTOTYPES),
        ] {
            let embeddings = embedder.embed_batch(texts)?;
            prototypes.insert(intent, embeddings);
        }

        Ok(Self {
            prototypes,
            default: QueryIntent::Implementation,
            threshold: 0.3,
        })
    }
}

/// Result of intent classification.
#[derive(Debug, Clone)]
pub struct ClassificationResult {
    pub intent: QueryIntent,
    /// Cosine similarity confidence. 0.0 = fell through to default.
    pub confidence: f32,
}

/// Classify query intent via cosine similarity against prototype embeddings.
///
/// For each intent, computes the maximum cosine similarity between the
/// query embedding and that intent's prototype embeddings.
/// Returns the intent with the highest max similarity.
/// Falls back to default if all similarities are below the threshold.
pub fn classify(query_embedding: &[f32], classifier: &IntentClassifier) -> ClassificationResult {
    let mut best_intent = classifier.default;
    let mut best_similarity: f32 = 0.0;

    for (intent, proto_embeddings) in &classifier.prototypes {
        let max_sim = proto_embeddings
            .iter()
            .map(|proto| cosine_similarity(query_embedding, proto))
            .fold(f32::NEG_INFINITY, f32::max);

        if max_sim > best_similarity {
            best_similarity = max_sim;
            best_intent = *intent;
        }
    }

    if best_similarity < classifier.threshold {
        return ClassificationResult {
            intent: classifier.default,
            confidence: 0.0,
        };
    }

    ClassificationResult {
        intent: best_intent,
        confidence: best_similarity,
    }
}

/// Compute cosine similarity between two vectors.
/// Returns 0.0 if either vector has zero magnitude.
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    debug_assert_eq!(a.len(), b.len(), "vectors must have same dimension");

    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let mag_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let mag_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

    if mag_a == 0.0 || mag_b == 0.0 {
        return 0.0;
    }

    dot / (mag_a * mag_b)
}

// --- Query Routing ---

/// Declarative routing table: maps each intent to retrieval limits.
/// Data, not code. New intents = new entries.
#[derive(Debug, Clone)]
pub struct RoutingTable {
    pub routes: HashMap<QueryIntent, RetrievalConfig>,
    pub default: RetrievalConfig,
}

impl Default for RoutingTable {
    fn default() -> Self {
        let mut routes = HashMap::new();

        // code_limit fixed at 5 (pre-V2.2 default) across all intents.
        // Differentiation is in supplementary context only.
        // Revisit once V3 quality harness measures recall@5 per intent.
        routes.insert(
            QueryIntent::Overview,
            RetrievalConfig {
                code_limit: 5,
                readme_limit: 3,
                crate_limit: 3,
                module_doc_limit: 3,
            },
        );

        routes.insert(
            QueryIntent::Implementation,
            RetrievalConfig {
                code_limit: 5,
                readme_limit: 1,
                crate_limit: 1,
                module_doc_limit: 2,
            },
        );

        routes.insert(
            QueryIntent::Relationship,
            RetrievalConfig {
                code_limit: 5,
                readme_limit: 1,
                crate_limit: 2,
                module_doc_limit: 2,
            },
        );

        routes.insert(
            QueryIntent::Comparison,
            RetrievalConfig {
                code_limit: 5,
                readme_limit: 2,
                crate_limit: 3,
                module_doc_limit: 2,
            },
        );

        Self {
            routes,
            default: RetrievalConfig::default(),
        }
    }
}

/// Look up retrieval limits for a classified intent.
/// Falls back to default if the intent is not in the routing table.
pub fn route(intent: QueryIntent, table: &RoutingTable) -> RetrievalConfig {
    table
        .routes
        .get(&intent)
        .cloned()
        .unwrap_or_else(|| table.default.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Cosine similarity unit tests (no model needed) ---

    #[test]
    fn test_cosine_similarity_identical() {
        let v = vec![1.0, 2.0, 3.0];
        let sim = cosine_similarity(&v, &v);
        assert!((sim - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_cosine_similarity_orthogonal() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![0.0, 1.0, 0.0];
        let sim = cosine_similarity(&a, &b);
        assert!(sim.abs() < 1e-6);
    }

    #[test]
    fn test_cosine_similarity_opposite() {
        let a = vec![1.0, 2.0, 3.0];
        let b = vec![-1.0, -2.0, -3.0];
        let sim = cosine_similarity(&a, &b);
        assert!((sim + 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_cosine_similarity_zero() {
        let a = vec![1.0, 2.0, 3.0];
        let b = vec![0.0, 0.0, 0.0];
        let sim = cosine_similarity(&a, &b);
        assert_eq!(sim, 0.0);
    }

    #[test]
    fn test_intent_serialization() {
        assert_eq!(
            serde_json::to_string(&QueryIntent::Overview).unwrap(),
            "\"overview\""
        );
        assert_eq!(
            serde_json::to_string(&QueryIntent::Implementation).unwrap(),
            "\"implementation\""
        );
    }

    // --- Routing tests ---

    #[test]
    fn test_route_overview() {
        let table = RoutingTable::default();
        let config = route(QueryIntent::Overview, &table);
        assert_eq!(config.code_limit, 5);
        assert_eq!(config.readme_limit, 3);
        assert_eq!(config.crate_limit, 3);
        assert_eq!(config.module_doc_limit, 3);
    }

    #[test]
    fn test_route_implementation() {
        let table = RoutingTable::default();
        let config = route(QueryIntent::Implementation, &table);
        assert_eq!(config.code_limit, 5);
        assert_eq!(config.readme_limit, 1);
    }

    #[test]
    fn test_route_relationship() {
        let table = RoutingTable::default();
        let config = route(QueryIntent::Relationship, &table);
        assert_eq!(config.code_limit, 5);
    }

    #[test]
    fn test_route_comparison() {
        let table = RoutingTable::default();
        let config = route(QueryIntent::Comparison, &table);
        assert_eq!(config.code_limit, 5);
        assert_eq!(config.crate_limit, 3);
    }

    #[test]
    fn test_route_unknown_uses_default() {
        // Empty routing table -> always falls back to default
        let table = RoutingTable {
            routes: HashMap::new(),
            default: RetrievalConfig {
                code_limit: 99,
                ..RetrievalConfig::default()
            },
        };
        let config = route(QueryIntent::Overview, &table);
        assert_eq!(config.code_limit, 99);
    }

    // --- Classification tests (require model download) ---

    #[test]
    #[ignore = "downloads model, run with --ignored"]
    fn test_classifier_build() {
        let mut embedder = Embedder::new().unwrap();
        let classifier = IntentClassifier::build(&mut embedder).unwrap();
        assert_eq!(classifier.prototypes.len(), 4);
    }

    #[test]
    #[ignore = "downloads model, run with --ignored"]
    fn test_classify_overview() {
        let mut embedder = Embedder::new().unwrap();
        let classifier = IntentClassifier::build(&mut embedder).unwrap();
        let embedding = embedder.embed_one("What is this project about?").unwrap();
        let result = classify(&embedding, &classifier);
        assert_eq!(result.intent, QueryIntent::Overview);
        assert!(result.confidence > 0.3);
    }

    #[test]
    #[ignore = "downloads model, run with --ignored"]
    fn test_classify_implementation() {
        let mut embedder = Embedder::new().unwrap();
        let classifier = IntentClassifier::build(&mut embedder).unwrap();
        let embedding = embedder.embed_one("How does the retriever work?").unwrap();
        let result = classify(&embedding, &classifier);
        assert_eq!(result.intent, QueryIntent::Implementation);
        assert!(result.confidence > 0.3);
    }

    #[test]
    #[ignore = "downloads model, run with --ignored"]
    fn test_classify_relationship() {
        let mut embedder = Embedder::new().unwrap();
        let classifier = IntentClassifier::build(&mut embedder).unwrap();
        let embedding = embedder
            .embed_one("What calls the retrieve function?")
            .unwrap();
        let result = classify(&embedding, &classifier);
        assert_eq!(result.intent, QueryIntent::Relationship);
        assert!(result.confidence > 0.3);
    }

    #[test]
    #[ignore = "downloads model, run with --ignored"]
    fn test_classify_comparison() {
        let mut embedder = Embedder::new().unwrap();
        let classifier = IntentClassifier::build(&mut embedder).unwrap();
        let embedding = embedder
            .embed_one("What are the differences between the retriever and the generator?")
            .unwrap();
        let result = classify(&embedding, &classifier);
        assert_eq!(result.intent, QueryIntent::Comparison);
        assert!(result.confidence > 0.3);
    }

    #[test]
    #[ignore = "downloads model, run with --ignored"]
    fn test_classify_paraphrase_implementation() {
        // This query would FAIL with keyword matching ("explain" → Overview)
        // Embedding similarity should correctly classify as Implementation
        let mut embedder = Embedder::new().unwrap();
        let classifier = IntentClassifier::build(&mut embedder).unwrap();
        let embedding = embedder
            .embed_one("Explain how the retriever implements caching")
            .unwrap();
        let result = classify(&embedding, &classifier);
        assert_eq!(result.intent, QueryIntent::Implementation);
    }

    #[test]
    #[ignore = "downloads model, run with --ignored"]
    fn test_classify_and_route_overview() {
        let mut embedder = Embedder::new().unwrap();
        let classifier = IntentClassifier::build(&mut embedder).unwrap();
        let routing = RoutingTable::default();

        let embedding = embedder.embed_one("What is code-raptor?").unwrap();
        let classification = classify(&embedding, &classifier);
        let config = route(classification.intent, &routing);

        assert_eq!(classification.intent, QueryIntent::Overview);
        assert_eq!(config.code_limit, 5);
        assert_eq!(config.crate_limit, 3);
    }
}
