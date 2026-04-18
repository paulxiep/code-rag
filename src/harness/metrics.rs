use std::time::Duration;

use serde::Serialize;

use super::dataset::TestCase;
use super::matching::evaluate_hits;
use super::runner::QueryResult;

/// Recall at K for a single query.
///
/// Denominator is `expected_files.len() + expected_identifiers.len()`.
/// These are independent dimensions: a single chunk can satisfy BOTH a file
/// expectation and an identifier expectation simultaneously. This is by design.
///
/// `expected_chunk_types`, `expected_projects`, `min_relevant_results`, and
/// `excluded_files` are NOT included in recall — they are separate coverage
/// checks reported in HitResult.
pub fn recall_at_k(result: &QueryResult, case: &TestCase, k: usize) -> f32 {
    let hits = evaluate_hits(result, case, k);
    let total_expected = case.expected_files.len() + case.expected_identifiers.len();
    if total_expected == 0 {
        return 1.0; // vacuous — no expectations
    }
    let total_hits = hits.file_hits.len() + hits.identifier_hits.len();
    total_hits as f32 / total_expected as f32
}

/// Recall over the entire retrieved pool (no top-k truncation).
///
/// In production, every chunk in `RetrievalResult` flows into `build_context`
/// and reaches the LLM. recall@5 measures rank quality, not pipeline success:
/// a chunk at rank 8 still reaches the LLM just like a chunk at rank 1.
/// This metric captures "did the retrieval pipeline surface the expected
/// chunk anywhere in its output" — the outcome that actually matters for
/// answer quality. Pair with MRR to retain rank-sensitivity signal.
pub fn recall_at_pool(result: &QueryResult, case: &TestCase) -> f32 {
    recall_at_k(result, case, result.retrieved.len())
}

/// Mean Reciprocal Rank: 1/rank of the first relevant result.
/// Returns 0.0 if no relevant result found.
pub fn mrr(result: &QueryResult, case: &TestCase) -> f32 {
    for item in &result.retrieved {
        let file_match = case
            .expected_files
            .iter()
            .any(|f| item.flat.file_path.contains(f));
        let id_match = case
            .expected_identifiers
            .iter()
            .any(|id| item.flat.identifier.as_deref() == Some(id));
        if file_match || id_match {
            return 1.0 / item.rank as f32;
        }
    }
    0.0
}

/// Compute the p-th percentile from a sorted slice of durations.
/// p is in [0.0, 1.0]. Uses nearest-rank method.
fn percentile(sorted_latencies: &[Duration], p: f32) -> Duration {
    if sorted_latencies.is_empty() {
        return Duration::ZERO;
    }
    let idx = ((p * sorted_latencies.len() as f32).ceil() as usize)
        .saturating_sub(1)
        .min(sorted_latencies.len() - 1);
    sorted_latencies[idx]
}

#[derive(Debug, Clone, Serialize)]
pub struct AggregateMetrics {
    pub total_queries: usize,
    pub recall_scored_queries: usize,
    pub no_expectation_queries: usize,
    pub recall_at_5: f32,
    pub recall_at_10: f32,
    /// Recall over the full retrieved pool (no top-k truncation) — the
    /// "did the expected chunk reach the LLM" signal. Complements r@5/r@10
    /// (rank quality) and MRR (first-hit rank).
    #[serde(default)]
    pub recall_at_pool: f32,
    pub mrr: f32,
    pub intent_accuracy: f32,
    pub latency_p50_ms: u64,
    pub latency_p95_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct IntentMetrics {
    pub intent: String,
    pub query_count: usize,
    pub recall_at_5: f32,
    pub recall_at_10: f32,
    #[serde(default)]
    pub recall_at_pool: f32,
    pub intent_accuracy: f32,
}

/// Compute aggregate metrics across all query results.
pub fn compute_aggregate(results: &[(QueryResult, &TestCase)]) -> AggregateMetrics {
    let total_queries = results.len();

    let mut recall_5_sum = 0.0f32;
    let mut recall_10_sum = 0.0f32;
    let mut recall_pool_sum = 0.0f32;
    let mut mrr_sum = 0.0f32;
    let mut recall_scored = 0usize;
    let mut intent_correct = 0usize;
    let mut intent_evaluated = 0usize;

    for (result, case) in results {
        let has_expectations =
            !case.expected_files.is_empty() || !case.expected_identifiers.is_empty();

        if has_expectations {
            recall_5_sum += recall_at_k(result, case, 5);
            recall_10_sum += recall_at_k(result, case, 10);
            recall_pool_sum += recall_at_pool(result, case);
            mrr_sum += mrr(result, case);
            recall_scored += 1;
        }

        if let Some(expected) = &case.expected_intent {
            intent_evaluated += 1;
            let parsed: Result<code_rag_engine::intent::QueryIntent, _> = expected.parse();
            if let Ok(ei) = parsed
                && ei == result.classified_intent
            {
                intent_correct += 1;
            }
        }
    }

    let mut latencies: Vec<Duration> = results.iter().map(|(r, _)| r.latency).collect();
    latencies.sort();

    AggregateMetrics {
        total_queries,
        recall_scored_queries: recall_scored,
        no_expectation_queries: total_queries - recall_scored,
        recall_at_5: if recall_scored > 0 {
            recall_5_sum / recall_scored as f32
        } else {
            0.0
        },
        recall_at_10: if recall_scored > 0 {
            recall_10_sum / recall_scored as f32
        } else {
            0.0
        },
        recall_at_pool: if recall_scored > 0 {
            recall_pool_sum / recall_scored as f32
        } else {
            0.0
        },
        mrr: if recall_scored > 0 {
            mrr_sum / recall_scored as f32
        } else {
            0.0
        },
        intent_accuracy: if intent_evaluated > 0 {
            intent_correct as f32 / intent_evaluated as f32
        } else {
            0.0
        },
        latency_p50_ms: percentile(&latencies, 0.5).as_millis() as u64,
        latency_p95_ms: percentile(&latencies, 0.95).as_millis() as u64,
    }
}

/// Break down metrics by intent category.
/// Groups by test_case.expected_intent (cases without expected_intent are excluded).
pub fn compute_by_intent(results: &[(QueryResult, &TestCase)]) -> Vec<IntentMetrics> {
    let mut groups: std::collections::HashMap<String, Vec<(&QueryResult, &TestCase)>> =
        std::collections::HashMap::new();

    for (result, case) in results {
        if let Some(intent) = &case.expected_intent {
            groups
                .entry(intent.clone())
                .or_default()
                .push((result, case));
        }
    }

    let mut metrics: Vec<IntentMetrics> = groups
        .into_iter()
        .map(|(intent, pairs)| {
            let query_count = pairs.len();
            let mut r5_sum = 0.0f32;
            let mut r10_sum = 0.0f32;
            let mut rpool_sum = 0.0f32;
            let mut scored = 0usize;
            let mut correct = 0usize;

            for (result, case) in &pairs {
                let has_expectations =
                    !case.expected_files.is_empty() || !case.expected_identifiers.is_empty();
                if has_expectations {
                    r5_sum += recall_at_k(result, case, 5);
                    r10_sum += recall_at_k(result, case, 10);
                    rpool_sum += recall_at_pool(result, case);
                    scored += 1;
                }

                let parsed: Result<code_rag_engine::intent::QueryIntent, _> = intent.parse();
                if let Ok(ei) = parsed
                    && ei == result.classified_intent
                {
                    correct += 1;
                }
            }

            IntentMetrics {
                intent,
                query_count,
                recall_at_5: if scored > 0 {
                    r5_sum / scored as f32
                } else {
                    0.0
                },
                recall_at_10: if scored > 0 {
                    r10_sum / scored as f32
                } else {
                    0.0
                },
                recall_at_pool: if scored > 0 {
                    rpool_sum / scored as f32
                } else {
                    0.0
                },
                intent_accuracy: correct as f32 / query_count as f32,
            }
        })
        .collect();

    metrics.sort_by(|a, b| a.intent.cmp(&b.intent));
    metrics
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::runner::RetrievedItem;
    use code_rag_engine::intent::QueryIntent;
    use code_rag_engine::retriever::FlatChunk;

    fn make_flat(file: &str, identifier: Option<&str>, relevance: f32) -> FlatChunk {
        FlatChunk {
            chunk_type: "code".to_string(),
            file_path: file.to_string(),
            identifier: identifier.map(String::from),
            project: "proj".to_string(),
            relevance,
            line: None,
        }
    }

    fn make_query_result(
        items: Vec<(FlatChunk, usize)>,
        intent: QueryIntent,
        latency_ms: u64,
    ) -> QueryResult {
        QueryResult {
            case_id: "test".to_string(),
            classified_intent: intent,
            confidence: 0.9,
            margin: 0.0,
            retrieved: items
                .into_iter()
                .map(|(flat, rank)| RetrievedItem { flat, rank })
                .collect(),
            latency: Duration::from_millis(latency_ms),
        }
    }

    fn make_test_case(files: Vec<&str>, identifiers: Vec<&str>, intent: Option<&str>) -> TestCase {
        TestCase {
            id: "test".to_string(),
            query: "test".to_string(),
            expected_intent: intent.map(String::from),
            expected_files: files.into_iter().map(String::from).collect(),
            expected_identifiers: identifiers.into_iter().map(String::from).collect(),
            expected_chunk_types: vec![],
            expected_projects: vec![],
            min_relevant_results: None,
            excluded_files: vec![],
            tags: vec![],
            notes: None,
        }
    }

    // --- recall_at_k tests ---

    #[test]
    fn test_recall_partial() {
        let result = make_query_result(
            vec![
                (make_flat("src/a.rs", Some("a"), 0.9), 1),
                (make_flat("src/b.rs", Some("b"), 0.8), 2),
            ],
            QueryIntent::Implementation,
            50,
        );
        let case = make_test_case(vec!["a.rs", "b.rs", "c.rs"], vec![], None);
        let r = recall_at_k(&result, &case, 5);
        assert!((r - 2.0 / 3.0).abs() < 1e-6);
    }

    #[test]
    fn test_recall_vacuous() {
        let result = make_query_result(vec![], QueryIntent::Overview, 50);
        let case = make_test_case(vec![], vec![], None);
        assert_eq!(recall_at_k(&result, &case, 5), 1.0);
    }

    #[test]
    fn test_recall_zero() {
        let result = make_query_result(
            vec![(make_flat("src/other.rs", None, 0.9), 1)],
            QueryIntent::Implementation,
            50,
        );
        let case = make_test_case(vec!["a.rs", "b.rs"], vec![], None);
        assert_eq!(recall_at_k(&result, &case, 5), 0.0);
    }

    #[test]
    fn test_recall_perfect() {
        let result = make_query_result(
            vec![
                (make_flat("src/a.rs", Some("foo"), 0.9), 1),
                (make_flat("src/b.rs", Some("bar"), 0.8), 2),
            ],
            QueryIntent::Implementation,
            50,
        );
        let case = make_test_case(vec!["a.rs", "b.rs"], vec!["foo"], None);
        assert_eq!(recall_at_k(&result, &case, 5), 1.0);
    }

    // --- MRR tests ---

    #[test]
    fn test_mrr_rank_1() {
        let result = make_query_result(
            vec![(make_flat("src/a.rs", None, 0.9), 1)],
            QueryIntent::Implementation,
            50,
        );
        let case = make_test_case(vec!["a.rs"], vec![], None);
        assert_eq!(mrr(&result, &case), 1.0);
    }

    #[test]
    fn test_mrr_rank_3() {
        let result = make_query_result(
            vec![
                (make_flat("src/other1.rs", None, 0.9), 1),
                (make_flat("src/other2.rs", None, 0.8), 2),
                (make_flat("src/a.rs", None, 0.7), 3),
            ],
            QueryIntent::Implementation,
            50,
        );
        let case = make_test_case(vec!["a.rs"], vec![], None);
        assert!((mrr(&result, &case) - 1.0 / 3.0).abs() < 1e-6);
    }

    #[test]
    fn test_mrr_none() {
        let result = make_query_result(
            vec![(make_flat("src/other.rs", None, 0.9), 1)],
            QueryIntent::Implementation,
            50,
        );
        let case = make_test_case(vec!["a.rs"], vec![], None);
        assert_eq!(mrr(&result, &case), 0.0);
    }

    // --- percentile tests ---

    #[test]
    fn test_percentile_p50() {
        let durations: Vec<Duration> = vec![10, 20, 30, 40, 50]
            .into_iter()
            .map(Duration::from_millis)
            .collect();
        let p = percentile(&durations, 0.5);
        assert_eq!(p, Duration::from_millis(30));
    }

    #[test]
    fn test_percentile_p95() {
        let durations: Vec<Duration> = vec![10, 20, 30, 40, 50]
            .into_iter()
            .map(Duration::from_millis)
            .collect();
        let p = percentile(&durations, 0.95);
        assert_eq!(p, Duration::from_millis(50));
    }

    #[test]
    fn test_percentile_empty() {
        let p = percentile(&[], 0.5);
        assert_eq!(p, Duration::ZERO);
    }

    // --- aggregate tests ---

    #[test]
    fn test_compute_aggregate_known() {
        let r1 = make_query_result(
            vec![(make_flat("src/a.rs", Some("foo"), 0.9), 1)],
            QueryIntent::Implementation,
            30,
        );
        let c1 = make_test_case(vec!["a.rs"], vec!["foo"], Some("implementation"));

        let r2 = make_query_result(
            vec![(make_flat("README.md", None, 0.8), 1)],
            QueryIntent::Overview,
            50,
        );
        let c2 = make_test_case(vec!["README.md"], vec![], Some("overview"));

        let pairs: Vec<(QueryResult, &TestCase)> = vec![(r1, &c1), (r2, &c2)];
        let agg = compute_aggregate(&pairs);

        assert_eq!(agg.total_queries, 2);
        assert_eq!(agg.recall_scored_queries, 2);
        assert_eq!(agg.no_expectation_queries, 0);
        assert_eq!(agg.recall_at_5, 1.0); // both perfect
        assert_eq!(agg.intent_accuracy, 1.0); // both correct
    }
}
