use serde::Serialize;
use std::collections::HashMap;

use super::config::RetrievalConfig;

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

impl std::str::FromStr for QueryIntent {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "overview" => Ok(Self::Overview),
            "implementation" => Ok(Self::Implementation),
            "relationship" => Ok(Self::Relationship),
            "comparison" => Ok(Self::Comparison),
            _ => Err(format!("unknown intent: {s}")),
        }
    }
}

// --- Prototype queries (static data, replaces keyword lists) ---

const OVERVIEW_PROTOTYPES: &[&str] = &[
    "What is this project?",
    "Tell me about this codebase",
    "Give me an overview",
    "What does this do?",
    "Describe the purpose",
    "What is the architecture?",
    "What is the purpose of this module?",
    "What is the role of this component?",
    "What is this package?",
];

const IMPLEMENTATION_PROTOTYPES: &[&str] = &[
    "How does this function work?",
    "Show me the implementation",
    "How is this implemented?",
    "Walk me through the logic",
    "How is this function implemented?",
    "Walk through this code step by step",
    "What are the steps of this algorithm?",
];

const RELATIONSHIP_PROTOTYPES: &[&str] = &[
    "What calls this function?",
    "How does A relate to B?",
    "What depends on this?",
    "Show me the call chain",
    "What uses this module?",
    "What formats does this support?",
    "How do errors propagate through the system?",
];

const COMPARISON_PROTOTYPES: &[&str] = &[
    "Compare A and B",
    "What are the differences between X and Y?",
    "How does A differ from B?",
    "A versus B",
    "Contrast these approaches",
    "What are the pros and cons?",
    "What is the difference between X and Y?",
    "How does X compare to Y?",
    "Differences between X and Y",
];

/// Pre-computed prototype embeddings for each intent.
/// Built once at startup; used for every classification call.
pub struct IntentClassifier {
    prototypes: HashMap<QueryIntent, Vec<Vec<f32>>>,
    default: QueryIntent,
    threshold: f32,
    /// If top1 - top2 margin is below this, fall back to default (ambiguous).
    /// 0.0 disables margin-based abstention.
    margin_threshold: f32,
    /// If Some(k), use top-k weighted voting across all prototypes instead of per-intent max.
    /// None = standard per-intent max-similarity classification.
    knn_k: Option<usize>,
}

impl IntentClassifier {
    /// Build the classifier by embedding all prototype queries.
    ///
    /// Accepts any embedding function — caller provides their platform-specific
    /// embedder as a closure. This keeps the engine crate free of I/O dependencies.
    pub fn build<E>(
        mut embed_batch: impl FnMut(&[&str]) -> Result<Vec<Vec<f32>>, E>,
    ) -> Result<Self, E> {
        let mut prototypes = HashMap::new();

        for (intent, texts) in [
            (QueryIntent::Overview, OVERVIEW_PROTOTYPES),
            (QueryIntent::Implementation, IMPLEMENTATION_PROTOTYPES),
            (QueryIntent::Relationship, RELATIONSHIP_PROTOTYPES),
            (QueryIntent::Comparison, COMPARISON_PROTOTYPES),
        ] {
            let embeddings = embed_batch(texts)?;
            prototypes.insert(intent, embeddings);
        }

        Ok(Self {
            prototypes,
            default: QueryIntent::Implementation,
            threshold: 0.3,
            margin_threshold: 0.0,
            knn_k: Some(3),
        })
    }

    /// Build from pre-computed prototype embeddings (e.g., loaded from exported data).
    pub fn from_prototypes(prototypes: HashMap<QueryIntent, Vec<Vec<f32>>>) -> Self {
        Self {
            prototypes,
            default: QueryIntent::Implementation,
            threshold: 0.3,
            margin_threshold: 0.0,
            knn_k: Some(3),
        }
    }

    /// Override the confidence threshold (cases below fall back to default).
    pub fn with_threshold(mut self, threshold: f32) -> Self {
        self.threshold = threshold;
        self
    }

    /// Override the margin threshold for abstention.
    /// If top1_sim - top2_sim < margin_threshold, fall back to default.
    pub fn with_margin_threshold(mut self, margin: f32) -> Self {
        self.margin_threshold = margin;
        self
    }

    /// Enable top-k weighted voting. Set to None for per-intent max (default).
    pub fn with_knn_k(mut self, k: Option<usize>) -> Self {
        self.knn_k = k;
        self
    }
}

/// Result of intent classification.
#[derive(Debug, Clone)]
pub struct ClassificationResult {
    pub intent: QueryIntent,
    /// Cosine similarity confidence. 0.0 = fell through to default.
    pub confidence: f32,
    /// Margin between top-1 and top-2 intent scores. Exposed for diagnostics.
    pub margin: f32,
}

/// Keyword pre-classifier: hard-override Comparison for dominant surface forms.
///
/// Returns `Some(Comparison)` when the query contains an unambiguous comparison
/// cue ("difference between", "differ from", " vs ", "compare"). Returns `None`
/// to defer to embedding-based classification.
///
/// Guards against adversarial false positives:
/// - "difference this/that/it makes" (idiomatic) → None
/// - "_vs_" / "-vs-" in file/identifier tokens → None
pub fn pre_classify_comparison(query: &str) -> Option<QueryIntent> {
    let q = query.to_lowercase();

    // Adversarial guards (return None = defer)
    if q.contains("difference this makes")
        || q.contains("difference that makes")
        || q.contains("difference it makes")
    {
        return None;
    }
    // "vs" inside a token (e.g. transformer_vs_rnn.py) — not a comparison cue
    let token_vs = q.contains("_vs_") || q.contains("_vs.") || q.contains("-vs-");

    // Positive cues
    // Match "differ" as a standalone token (not inside "different" idioms covered above).
    let has_differ = q.split(|c: char| !c.is_alphanumeric())
        .any(|tok| tok == "differ" || tok == "differs");
    let strong_cue = q.contains("difference between")
        || q.contains("differences between")
        || q.contains(" differ from ")
        || has_differ
        || q.contains("compare ")
        || (q.contains(" vs ") && !token_vs)
        || (q.contains(" vs. ") && !token_vs);

    if strong_cue {
        Some(QueryIntent::Comparison)
    } else {
        None
    }
}

/// Classify query intent via cosine similarity against prototype embeddings.
///
/// For each intent, computes the maximum cosine similarity between the
/// query embedding and that intent's prototype embeddings.
/// Returns the intent with the highest max similarity.
/// Falls back to default if top similarity is below `threshold`, OR if the
/// margin between top-1 and top-2 intents is below `margin_threshold`.
pub fn classify(query_embedding: &[f32], classifier: &IntentClassifier) -> ClassificationResult {
    // Collect per-intent scores. Two modes:
    // - knn_k=None: per-intent max similarity (default)
    // - knn_k=Some(k): flatten all prototypes, take top-k by similarity, sum their
    //   similarities per intent (weighted vote). More robust to single noisy prototypes.
    let mut scores: Vec<(QueryIntent, f32)> = if let Some(k) = classifier.knn_k {
        let mut flat: Vec<(QueryIntent, f32)> = classifier
            .prototypes
            .iter()
            .flat_map(|(intent, protos)| {
                protos
                    .iter()
                    .map(move |p| (*intent, cosine_similarity(query_embedding, p)))
            })
            .collect();
        flat.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        let mut votes: HashMap<QueryIntent, f32> = HashMap::new();
        for (intent, sim) in flat.into_iter().take(k) {
            *votes.entry(intent).or_insert(0.0) += sim;
        }
        // Ensure all intents are represented so sorting still finds top-1/top-2
        for intent in [
            QueryIntent::Overview,
            QueryIntent::Implementation,
            QueryIntent::Relationship,
            QueryIntent::Comparison,
        ] {
            votes.entry(intent).or_insert(0.0);
        }
        votes.into_iter().collect()
    } else {
        classifier
            .prototypes
            .iter()
            .map(|(intent, proto_embeddings)| {
                let max_sim = proto_embeddings
                    .iter()
                    .map(|proto| cosine_similarity(query_embedding, proto))
                    .fold(f32::NEG_INFINITY, f32::max);
                (*intent, max_sim)
            })
            .collect()
    };

    // Sort descending by similarity
    scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    let (top_intent, top_sim) = scores[0];
    let top2_sim = scores.get(1).map(|(_, s)| *s).unwrap_or(0.0);
    let margin = top_sim - top2_sim;

    // Threshold-based abstention
    if top_sim < classifier.threshold {
        return ClassificationResult {
            intent: classifier.default,
            confidence: 0.0,
            margin,
        };
    }

    // Margin-based abstention
    if classifier.margin_threshold > 0.0 && margin < classifier.margin_threshold {
        return ClassificationResult {
            intent: classifier.default,
            confidence: top_sim,
            margin,
        };
    }

    ClassificationResult {
        intent: top_intent,
        confidence: top_sim,
        margin,
    }
}

/// Compute cosine similarity between two vectors.
/// Returns 0.0 if either vector has zero magnitude.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
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

    // --- Classification tests with mock embedder ---

    #[test]
    fn test_classifier_build_with_closure() {
        // Mock embedder that returns fixed-dimension vectors
        let classifier = IntentClassifier::build(|texts: &[&str]| {
            Ok::<_, String>(texts.iter().map(|_| vec![0.1; 384]).collect())
        })
        .unwrap();
        assert_eq!(classifier.prototypes.len(), 4);
    }

    #[test]
    fn test_classifier_build_propagates_error() {
        let result =
            IntentClassifier::build(|_texts: &[&str]| Err::<Vec<Vec<f32>>, _>("mock embed error"));
        assert!(result.is_err());
    }

    #[test]
    fn test_classify_below_threshold_returns_default() {
        // Build with zero vectors so all similarities are 0
        let classifier = IntentClassifier::build(|texts: &[&str]| {
            Ok::<_, String>(texts.iter().map(|_| vec![0.0; 3]).collect())
        })
        .unwrap();

        let query = vec![1.0, 0.0, 0.0];
        let result = classify(&query, &classifier);
        assert_eq!(result.intent, QueryIntent::Implementation); // default
        assert_eq!(result.confidence, 0.0);
    }

    // --- B4: pre_classify_comparison tests ---

    #[test]
    fn test_pre_classify_difference_between() {
        assert_eq!(
            pre_classify_comparison("What is the difference between X and Y?"),
            Some(QueryIntent::Comparison)
        );
        assert_eq!(
            pre_classify_comparison("Differences between A and B"),
            Some(QueryIntent::Comparison)
        );
    }

    #[test]
    fn test_pre_classify_differ() {
        assert_eq!(
            pre_classify_comparison("How does A differ from B?"),
            Some(QueryIntent::Comparison)
        );
        assert_eq!(
            pre_classify_comparison("How do the handlers differ?"),
            Some(QueryIntent::Comparison)
        );
    }

    #[test]
    fn test_pre_classify_compare_and_vs() {
        assert_eq!(
            pre_classify_comparison("Compare A and B"),
            Some(QueryIntent::Comparison)
        );
        assert_eq!(
            pre_classify_comparison("X vs Y"),
            Some(QueryIntent::Comparison)
        );
    }

    #[test]
    fn test_pre_classify_adversarial_idiom_returns_none() {
        // Idiomatic "difference" — not a comparison intent
        assert_eq!(
            pre_classify_comparison("What is the difference this makes?"),
            None
        );
    }

    #[test]
    fn test_pre_classify_adversarial_vs_in_filename_returns_none() {
        // "vs" inside an identifier/filename token is not a comparison cue
        assert_eq!(
            pre_classify_comparison("How does transformer_vs_rnn.py work?"),
            None
        );
    }

    #[test]
    fn test_pre_classify_non_comparison_returns_none() {
        assert_eq!(pre_classify_comparison("What is invoice-parse?"), None);
        assert_eq!(
            pre_classify_comparison("How does the retriever work?"),
            None
        );
        assert_eq!(
            pre_classify_comparison("What calls this function?"),
            None
        );
    }

    #[test]
    fn test_classify_exposes_margin() {
        // Build classifier where Overview protos match query strongly, others don't.
        // Prototypes are iterated in the constructor order of the HashMap entries,
        // but we only check the margin field exists and is non-negative.
        let classifier = IntentClassifier::build(|texts: &[&str]| {
            Ok::<_, String>(texts.iter().map(|_| vec![0.5; 4]).collect())
        })
        .unwrap();
        let query = vec![1.0, 0.0, 0.0, 0.0];
        let result = classify(&query, &classifier);
        assert!(result.margin >= 0.0);
    }

    // --- FromStr tests ---

    #[test]
    fn test_from_str_all_variants() {
        assert_eq!(
            "overview".parse::<QueryIntent>().unwrap(),
            QueryIntent::Overview
        );
        assert_eq!(
            "implementation".parse::<QueryIntent>().unwrap(),
            QueryIntent::Implementation
        );
        assert_eq!(
            "relationship".parse::<QueryIntent>().unwrap(),
            QueryIntent::Relationship
        );
        assert_eq!(
            "comparison".parse::<QueryIntent>().unwrap(),
            QueryIntent::Comparison
        );
    }

    #[test]
    fn test_from_str_invalid() {
        assert!("nonsense".parse::<QueryIntent>().is_err());
        assert!("Overview".parse::<QueryIntent>().is_err()); // case-sensitive
        assert!("".parse::<QueryIntent>().is_err());
    }
}
