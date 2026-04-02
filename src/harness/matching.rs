use code_rag_engine::retriever::FlatChunk;

use super::dataset::TestCase;
use super::runner::QueryResult;

/// Substring match: "retriever.rs" matches "src/engine/retriever.rs"
pub fn matches_file(chunk: &FlatChunk, expected: &str) -> bool {
    chunk.file_path.contains(expected)
}

/// Exact match against identifier
pub fn matches_identifier(chunk: &FlatChunk, expected: &str) -> bool {
    chunk.identifier.as_deref() == Some(expected)
}

/// Exact match against chunk type
pub fn matches_chunk_type(chunk: &FlatChunk, expected: &str) -> bool {
    chunk.chunk_type == expected
}

/// Exact match against project name
pub fn matches_project(chunk: &FlatChunk, expected: &str) -> bool {
    chunk.project == expected
}

/// Substring match for excluded files — returns true if chunk matches an exclusion
pub fn matches_excluded_file(chunk: &FlatChunk, excluded: &str) -> bool {
    chunk.file_path.contains(excluded)
}

/// Result of evaluating a single test case against retrieval results.
#[derive(Debug, Clone)]
pub struct HitResult {
    pub file_hits: Vec<String>,
    pub file_misses: Vec<String>,
    pub identifier_hits: Vec<String>,
    pub identifier_misses: Vec<String>,
    pub chunk_type_hits: Vec<String>,
    pub chunk_type_misses: Vec<String>,
    pub project_hits: Vec<String>,
    pub project_misses: Vec<String>,
    pub intent_correct: Option<bool>,
    pub excluded_file_violations: Vec<String>,
    pub min_relevant_met: Option<bool>,
    pub relevant_count: usize,
}

/// Evaluate hits for a query result against a test case, using top-K items.
pub fn evaluate_hits(result: &QueryResult, case: &TestCase, k: usize) -> HitResult {
    let top_k: Vec<&FlatChunk> = result.retrieved[..k.min(result.retrieved.len())]
        .iter()
        .map(|item| &item.flat)
        .collect();

    let mut file_hits = Vec::new();
    let mut file_misses = Vec::new();
    for expected in &case.expected_files {
        if top_k.iter().any(|c| matches_file(c, expected)) {
            file_hits.push(expected.clone());
        } else {
            file_misses.push(expected.clone());
        }
    }

    let mut identifier_hits = Vec::new();
    let mut identifier_misses = Vec::new();
    for expected in &case.expected_identifiers {
        if top_k.iter().any(|c| matches_identifier(c, expected)) {
            identifier_hits.push(expected.clone());
        } else {
            identifier_misses.push(expected.clone());
        }
    }

    let mut chunk_type_hits = Vec::new();
    let mut chunk_type_misses = Vec::new();
    for expected in &case.expected_chunk_types {
        if top_k.iter().any(|c| matches_chunk_type(c, expected)) {
            chunk_type_hits.push(expected.clone());
        } else {
            chunk_type_misses.push(expected.clone());
        }
    }

    let mut project_hits = Vec::new();
    let mut project_misses = Vec::new();
    for expected in &case.expected_projects {
        if top_k.iter().any(|c| matches_project(c, expected)) {
            project_hits.push(expected.clone());
        } else {
            project_misses.push(expected.clone());
        }
    }

    let intent_correct = case.expected_intent.as_ref().map(|expected| {
        let expected_intent: Result<code_rag_engine::intent::QueryIntent, _> = expected.parse();
        expected_intent
            .map(|ei| ei == result.classified_intent)
            .unwrap_or(false)
    });

    let mut excluded_file_violations = Vec::new();
    for excluded in &case.excluded_files {
        if top_k.iter().any(|c| matches_excluded_file(c, excluded)) {
            excluded_file_violations.push(excluded.clone());
        }
    }

    let relevant_count = top_k.iter().filter(|c| c.relevance > 0.5).count();
    let min_relevant_met = case.min_relevant_results.map(|min| relevant_count >= min);

    HitResult {
        file_hits,
        file_misses,
        identifier_hits,
        identifier_misses,
        chunk_type_hits,
        chunk_type_misses,
        project_hits,
        project_misses,
        intent_correct,
        excluded_file_violations,
        min_relevant_met,
        relevant_count,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::runner::RetrievedItem;
    use code_rag_engine::intent::QueryIntent;
    use std::time::Duration;

    fn make_flat(
        chunk_type: &str,
        file_path: &str,
        identifier: Option<&str>,
        project: &str,
        relevance: f32,
    ) -> FlatChunk {
        FlatChunk {
            chunk_type: chunk_type.to_string(),
            file_path: file_path.to_string(),
            identifier: identifier.map(|s| s.to_string()),
            project: project.to_string(),
            relevance,
            line: None,
        }
    }

    fn make_item(flat: FlatChunk, rank: usize) -> RetrievedItem {
        RetrievedItem { flat, rank }
    }

    fn make_result(items: Vec<RetrievedItem>) -> QueryResult {
        QueryResult {
            case_id: "test".to_string(),
            classified_intent: QueryIntent::Implementation,
            confidence: 0.9,
            retrieved: items,
            latency: Duration::from_millis(50),
        }
    }

    fn make_case(
        files: Vec<&str>,
        identifiers: Vec<&str>,
        chunk_types: Vec<&str>,
        projects: Vec<&str>,
    ) -> TestCase {
        TestCase {
            id: "test".to_string(),
            query: "test query".to_string(),
            expected_intent: Some("implementation".to_string()),
            expected_files: files.into_iter().map(String::from).collect(),
            expected_identifiers: identifiers.into_iter().map(String::from).collect(),
            expected_chunk_types: chunk_types.into_iter().map(String::from).collect(),
            expected_projects: projects.into_iter().map(String::from).collect(),
            min_relevant_results: None,
            excluded_files: vec![],
            tags: vec![],
            notes: None,
        }
    }

    // --- Match function tests ---

    #[test]
    fn test_matches_file_positive() {
        let chunk = make_flat("code", "src/engine/retriever.rs", None, "p", 0.9);
        assert!(matches_file(&chunk, "retriever.rs"));
    }

    #[test]
    fn test_matches_file_negative() {
        let chunk = make_flat("code", "src/engine/retriever.rs", None, "p", 0.9);
        assert!(!matches_file(&chunk, "context.rs"));
    }

    #[test]
    fn test_matches_file_exact() {
        let chunk = make_flat("code", "retriever.rs", None, "p", 0.9);
        assert!(matches_file(&chunk, "retriever.rs"));
    }

    #[test]
    fn test_matches_identifier_match() {
        let chunk = make_flat("code", "f.rs", Some("retrieve"), "p", 0.9);
        assert!(matches_identifier(&chunk, "retrieve"));
    }

    #[test]
    fn test_matches_identifier_no_match() {
        let chunk = make_flat("code", "f.rs", Some("search"), "p", 0.9);
        assert!(!matches_identifier(&chunk, "retrieve"));
    }

    #[test]
    fn test_matches_identifier_none() {
        let chunk = make_flat("readme", "README.md", None, "p", 0.9);
        assert!(!matches_identifier(&chunk, "retrieve"));
    }

    #[test]
    fn test_matches_project_positive() {
        let chunk = make_flat("code", "f.rs", None, "code-rag-chat", 0.9);
        assert!(matches_project(&chunk, "code-rag-chat"));
    }

    #[test]
    fn test_matches_project_negative() {
        let chunk = make_flat("code", "f.rs", None, "code-rag-chat", 0.9);
        assert!(!matches_project(&chunk, "code-raptor"));
    }

    #[test]
    fn test_matches_excluded_file_positive() {
        let chunk = make_flat("code", "src/engine/retriever.rs", None, "p", 0.9);
        assert!(matches_excluded_file(&chunk, "retriever.rs"));
    }

    #[test]
    fn test_matches_excluded_file_negative() {
        let chunk = make_flat("code", "src/engine/retriever.rs", None, "p", 0.9);
        assert!(!matches_excluded_file(&chunk, "context.rs"));
    }

    // --- evaluate_hits tests ---

    #[test]
    fn test_evaluate_hits_all_hits() {
        let items = vec![
            make_item(
                make_flat(
                    "code",
                    "src/engine/retriever.rs",
                    Some("retrieve"),
                    "proj",
                    0.9,
                ),
                1,
            ),
            make_item(
                make_flat("code", "src/api/handlers.rs", Some("chat"), "proj", 0.8),
                2,
            ),
            make_item(make_flat("readme", "README.md", None, "proj", 0.7), 3),
        ];
        let result = make_result(items);
        let case = make_case(
            vec!["retriever.rs", "handlers.rs"],
            vec!["retrieve"],
            vec![],
            vec![],
        );

        let hits = evaluate_hits(&result, &case, 10);
        assert_eq!(hits.file_hits.len(), 2);
        assert_eq!(hits.file_misses.len(), 0);
        assert_eq!(hits.identifier_hits.len(), 1);
        assert_eq!(hits.identifier_misses.len(), 0);
    }

    #[test]
    fn test_evaluate_hits_all_misses() {
        let items = vec![make_item(
            make_flat("code", "src/other.rs", Some("other"), "proj", 0.9),
            1,
        )];
        let result = make_result(items);
        let case = make_case(
            vec!["retriever.rs", "handlers.rs", "context.rs"],
            vec![],
            vec![],
            vec![],
        );

        let hits = evaluate_hits(&result, &case, 10);
        assert_eq!(hits.file_hits.len(), 0);
        assert_eq!(hits.file_misses.len(), 3);
    }

    #[test]
    fn test_evaluate_hits_partial() {
        let items = vec![make_item(
            make_flat(
                "code",
                "src/engine/retriever.rs",
                Some("retrieve"),
                "proj",
                0.9,
            ),
            1,
        )];
        let result = make_result(items);
        let case = make_case(
            vec!["retriever.rs", "handlers.rs", "context.rs"],
            vec![],
            vec![],
            vec![],
        );

        let hits = evaluate_hits(&result, &case, 10);
        assert_eq!(hits.file_hits.len(), 1);
        assert_eq!(hits.file_misses.len(), 2);
    }

    #[test]
    fn test_evaluate_hits_empty_expectations() {
        let items = vec![make_item(
            make_flat("code", "src/a.rs", Some("a"), "proj", 0.9),
            1,
        )];
        let result = make_result(items);
        let case = make_case(vec![], vec![], vec![], vec![]);

        let hits = evaluate_hits(&result, &case, 10);
        assert_eq!(hits.file_hits.len(), 0);
        assert_eq!(hits.file_misses.len(), 0);
        assert_eq!(hits.identifier_hits.len(), 0);
        assert_eq!(hits.identifier_misses.len(), 0);
    }

    #[test]
    fn test_evaluate_hits_dedup_same_file_two_chunk_types() {
        // Same file in code and module_doc chunks — counts as 1 file hit
        let items = vec![
            make_item(
                make_flat(
                    "code",
                    "src/engine/retriever.rs",
                    Some("retrieve"),
                    "proj",
                    0.9,
                ),
                1,
            ),
            make_item(
                make_flat(
                    "module_doc",
                    "src/engine/retriever.rs",
                    Some("retriever"),
                    "proj",
                    0.8,
                ),
                2,
            ),
        ];
        let result = make_result(items);
        let case = make_case(vec!["retriever.rs"], vec![], vec![], vec![]);

        let hits = evaluate_hits(&result, &case, 10);
        assert_eq!(hits.file_hits.len(), 1); // dedup by expected value
    }

    #[test]
    fn test_evaluate_hits_excluded_violation() {
        let items = vec![make_item(
            make_flat(
                "code",
                "src/engine/retriever.rs",
                Some("retrieve"),
                "proj",
                0.9,
            ),
            1,
        )];
        let result = make_result(items);
        let mut case = make_case(vec![], vec![], vec![], vec![]);
        case.excluded_files = vec!["retriever.rs".to_string()];

        let hits = evaluate_hits(&result, &case, 10);
        assert_eq!(hits.excluded_file_violations.len(), 1);
        assert_eq!(hits.excluded_file_violations[0], "retriever.rs");
    }

    #[test]
    fn test_evaluate_hits_excluded_clean() {
        let items = vec![make_item(
            make_flat("code", "src/api/handlers.rs", Some("chat"), "proj", 0.9),
            1,
        )];
        let result = make_result(items);
        let mut case = make_case(vec![], vec![], vec![], vec![]);
        case.excluded_files = vec!["retriever.rs".to_string()];

        let hits = evaluate_hits(&result, &case, 10);
        assert!(hits.excluded_file_violations.is_empty());
    }

    #[test]
    fn test_evaluate_hits_min_relevant_met() {
        let items = vec![
            make_item(make_flat("code", "a.rs", None, "p", 0.8), 1),
            make_item(make_flat("code", "b.rs", None, "p", 0.7), 2),
            make_item(make_flat("code", "c.rs", None, "p", 0.6), 3),
            make_item(make_flat("code", "d.rs", None, "p", 0.3), 4), // below 0.5
        ];
        let result = make_result(items);
        let mut case = make_case(vec![], vec![], vec![], vec![]);
        case.min_relevant_results = Some(3);

        let hits = evaluate_hits(&result, &case, 10);
        assert_eq!(hits.min_relevant_met, Some(true));
        assert_eq!(hits.relevant_count, 3);
    }

    #[test]
    fn test_evaluate_hits_min_relevant_not_met() {
        let items = vec![
            make_item(make_flat("code", "a.rs", None, "p", 0.6), 1),
            make_item(make_flat("code", "b.rs", None, "p", 0.3), 2),
        ];
        let result = make_result(items);
        let mut case = make_case(vec![], vec![], vec![], vec![]);
        case.min_relevant_results = Some(3);

        let hits = evaluate_hits(&result, &case, 10);
        assert_eq!(hits.min_relevant_met, Some(false));
        assert_eq!(hits.relevant_count, 1);
    }

    #[test]
    fn test_evaluate_hits_min_relevant_absent() {
        let result = make_result(vec![]);
        let case = make_case(vec![], vec![], vec![], vec![]);

        let hits = evaluate_hits(&result, &case, 10);
        assert_eq!(hits.min_relevant_met, None);
    }

    #[test]
    fn test_evaluate_hits_project_hit() {
        let items = vec![make_item(
            make_flat("code", "a.rs", None, "code-rag-chat", 0.9),
            1,
        )];
        let result = make_result(items);
        let case = make_case(vec![], vec![], vec![], vec!["code-rag-chat"]);

        let hits = evaluate_hits(&result, &case, 10);
        assert_eq!(hits.project_hits, vec!["code-rag-chat"]);
        assert!(hits.project_misses.is_empty());
    }

    #[test]
    fn test_evaluate_hits_project_miss() {
        let items = vec![make_item(
            make_flat("code", "a.rs", None, "code-rag-chat", 0.9),
            1,
        )];
        let result = make_result(items);
        let case = make_case(vec![], vec![], vec![], vec!["code-raptor"]);

        let hits = evaluate_hits(&result, &case, 10);
        assert!(hits.project_hits.is_empty());
        assert_eq!(hits.project_misses, vec!["code-raptor"]);
    }

    #[test]
    fn test_evaluate_hits_intent_correct() {
        let result = make_result(vec![]); // classified_intent = Implementation
        let mut case = make_case(vec![], vec![], vec![], vec![]);
        case.expected_intent = Some("implementation".to_string());

        let hits = evaluate_hits(&result, &case, 10);
        assert_eq!(hits.intent_correct, Some(true));
    }

    #[test]
    fn test_evaluate_hits_intent_incorrect() {
        let result = make_result(vec![]); // classified_intent = Implementation
        let mut case = make_case(vec![], vec![], vec![], vec![]);
        case.expected_intent = Some("overview".to_string());

        let hits = evaluate_hits(&result, &case, 10);
        assert_eq!(hits.intent_correct, Some(false));
    }

    #[test]
    fn test_evaluate_hits_intent_absent() {
        let result = make_result(vec![]);
        let mut case = make_case(vec![], vec![], vec![], vec![]);
        case.expected_intent = None;

        let hits = evaluate_hits(&result, &case, 10);
        assert_eq!(hits.intent_correct, None);
    }
}
