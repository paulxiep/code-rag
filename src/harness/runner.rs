use std::time::{Duration, Instant};

use code_rag_engine::config::EngineConfig;
use code_rag_engine::intent::{self, IntentClassifier, QueryIntent};
use code_rag_engine::retriever::{FlatChunk, RetrievalResult};

use crate::engine::retriever::retrieve;
use crate::store::{Embedder, VectorStore};

use super::dataset::TestCase;

/// Raw result from running a single test query.
#[derive(Debug, Clone)]
pub struct QueryResult {
    /// Test case ID (for joining with TestCase)
    pub case_id: String,

    /// Intent from classifier (or ground-truth)
    pub classified_intent: QueryIntent,

    /// Cosine similarity confidence from classifier
    pub confidence: f32,

    /// All retrieved items, flattened across chunk types, sorted by relevance desc
    pub retrieved: Vec<RetrievedItem>,

    /// Wall-clock time for embed + classify + retrieve
    pub latency: Duration,
}

/// A single retrieved item for harness evaluation.
/// Wraps `FlatChunk` with a 1-indexed rank.
#[derive(Debug, Clone)]
pub struct RetrievedItem {
    /// The flattened chunk from RetrievalResult::flatten()
    pub flat: FlatChunk,

    /// 1-indexed position in sorted results
    pub rank: usize,
}

/// Convert a RetrievalResult into ranked RetrievedItems.
pub fn to_retrieved_items(result: &RetrievalResult) -> Vec<RetrievedItem> {
    result
        .flatten()
        .into_iter()
        .enumerate()
        .map(|(i, flat)| RetrievedItem {
            flat,
            rank: i + 1, // 1-indexed
        })
        .collect()
}

/// Run all test cases against the retrieval pipeline.
///
/// If `ground_truth` is true, uses `expected_intent` from each test case for routing
/// instead of the classifier. Missing `expected_intent` in ground-truth mode is a hard error.
pub async fn run_all(
    cases: &[TestCase],
    embedder: &mut Embedder,
    classifier: &IntentClassifier,
    store: &VectorStore,
    config: &EngineConfig,
    ground_truth: bool,
    verbose: bool,
) -> anyhow::Result<Vec<QueryResult>> {
    let mut results = Vec::with_capacity(cases.len());

    for (i, case) in cases.iter().enumerate() {
        if verbose {
            println!("Running query {}/{}: {}...", i + 1, cases.len(), case.id);
        }

        let start = Instant::now();

        // 1. Embed query
        let embedding = embedder.embed_one(&case.query)?;

        // 2. Classify or use ground-truth intent
        let (classified_intent, confidence) = if ground_truth {
            let intent_str = case.expected_intent.as_deref().ok_or_else(|| {
                anyhow::anyhow!(
                    "Ground-truth mode: test case '{}' missing expected_intent",
                    case.id
                )
            })?;
            let intent: QueryIntent = intent_str
                .parse()
                .map_err(|e: String| anyhow::anyhow!("{}", e))?;
            (intent, 1.0)
        } else {
            let cr = intent::classify(&embedding, classifier);
            (cr.intent, cr.confidence)
        };

        // 3. Route
        let retrieval_config = intent::route(classified_intent, &config.routing);

        // 4. Retrieve
        let retrieval_result =
            retrieve(&embedding, store, &retrieval_config, classified_intent).await?;

        let latency = start.elapsed();

        // 5. Flatten and rank
        let retrieved = to_retrieved_items(&retrieval_result);

        results.push(QueryResult {
            case_id: case.id.clone(),
            classified_intent,
            confidence,
            retrieved,
            latency,
        });
    }

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use code_rag_engine::retriever::ScoredChunk;
    use code_rag_types::CodeChunk;

    fn make_code_chunk(file: &str, id: &str, score: f32) -> ScoredChunk<CodeChunk> {
        ScoredChunk {
            chunk: CodeChunk {
                file_path: file.to_string(),
                language: "rust".to_string(),
                identifier: id.to_string(),
                node_type: "function_definition".to_string(),
                code_content: "fn test() {}".to_string(),
                start_line: 1,
                project_name: "proj".to_string(),
                docstring: None,
                chunk_id: "id".to_string(),
                content_hash: "hash".to_string(),
                embedding_model_version: "test".to_string(),
            },
            score,
        }
    }

    #[test]
    fn test_to_retrieved_items_ranking() {
        let result = RetrievalResult {
            code_chunks: vec![
                make_code_chunk("src/a.rs", "a", 0.9),
                make_code_chunk("src/b.rs", "b", 0.7),
                make_code_chunk("src/c.rs", "c", 0.5),
            ],
            readme_chunks: vec![],
            crate_chunks: vec![],
            module_doc_chunks: vec![],
            intent: QueryIntent::Implementation,
        };

        let items = to_retrieved_items(&result);
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].rank, 1);
        assert_eq!(items[0].flat.file_path, "src/a.rs");
        assert_eq!(items[1].rank, 2);
        assert_eq!(items[2].rank, 3);
    }

    #[test]
    fn test_to_retrieved_items_empty() {
        let result = RetrievalResult {
            code_chunks: vec![],
            readme_chunks: vec![],
            crate_chunks: vec![],
            module_doc_chunks: vec![],
            intent: QueryIntent::Overview,
        };

        let items = to_retrieved_items(&result);
        assert!(items.is_empty());
    }
}
