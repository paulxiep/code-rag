use std::path::Path;

use serde::{Deserialize, Serialize};

const VALID_INTENTS: &[&str] = &["overview", "implementation", "relationship", "comparison"];

#[derive(Debug, Serialize, Deserialize)]
pub struct TestDataset {
    /// Human-readable purpose of this dataset
    pub description: String,

    /// Schema version — starts at 1, bump on breaking changes
    pub schema_version: u32,

    /// The test cases
    pub cases: Vec<TestCase>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestCase {
    // --- Identity ---
    /// Unique key for reporting, e.g. "overview-01", "hero-retriever"
    pub id: String,

    /// Natural language query to evaluate
    pub query: String,

    // --- Expectations (all optional via #[serde(default)]) ---
    /// Ground-truth intent classification.
    /// Values: "overview" | "implementation" | "relationship" | "comparison"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_intent: Option<String>,

    /// Expected file paths in retrieval results (substring match).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub expected_files: Vec<String>,

    /// Expected function/class/struct names in results (exact match).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub expected_identifiers: Vec<String>,

    /// Expected chunk types that should appear in results (exact match).
    /// Values: "code" | "readme" | "crate" | "module_doc"
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub expected_chunk_types: Vec<String>,

    /// Expected project names in results (exact match).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub expected_projects: Vec<String>,

    // --- Soft expectations (pipeline-agnostic) ---
    /// Minimum number of results with relevance > 0.5 expected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_relevant_results: Option<usize>,

    /// File paths that should NOT appear in results (substring match).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub excluded_files: Vec<String>,

    // --- Metadata ---
    /// Tags for filtering and categorization.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,

    /// Free-form notes explaining why this test case exists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

impl TestDataset {
    /// Load and deserialize from a JSON file.
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let dataset: Self = serde_json::from_str(&content)?;
        Ok(dataset)
    }

    /// Filter cases by tag.
    pub fn filter_by_tag(&self, tag: &str) -> Vec<&TestCase> {
        self.cases
            .iter()
            .filter(|c| c.tags.iter().any(|t| t == tag))
            .collect()
    }

    /// Structural validation. Returns warnings, not errors.
    pub fn validate(&self) -> Vec<String> {
        let mut warnings = Vec::new();

        if self.cases.is_empty() {
            warnings.push("Dataset has zero test cases".to_string());
        }

        let mut seen_ids = std::collections::HashSet::new();
        for case in &self.cases {
            if case.id.is_empty() {
                warnings.push("Test case has empty id".to_string());
            }
            if !seen_ids.insert(&case.id) {
                warnings.push(format!("Duplicate test case id: {}", case.id));
            }
            if let Some(intent) = &case.expected_intent
                && !VALID_INTENTS.contains(&intent.as_str())
            {
                warnings.push(format!(
                    "Unknown intent '{}' in test case '{}'",
                    intent, case.id
                ));
            }
        }

        warnings
    }

    /// Strict validation for CI and baseline runs.
    /// Calls validate() and promotes warnings to errors.
    pub fn validate_strict(&self) -> anyhow::Result<()> {
        let warnings = self.validate();
        if !warnings.is_empty() {
            anyhow::bail!("Dataset validation failed:\n{}", warnings.join("\n"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_case(id: &str, query: &str) -> TestCase {
        TestCase {
            id: id.to_string(),
            query: query.to_string(),
            expected_intent: None,
            expected_files: vec![],
            expected_identifiers: vec![],
            expected_chunk_types: vec![],
            expected_projects: vec![],
            min_relevant_results: None,
            excluded_files: vec![],
            tags: vec![],
            notes: None,
        }
    }

    fn sample_dataset() -> TestDataset {
        TestDataset {
            description: "Test".to_string(),
            schema_version: 1,
            cases: vec![
                TestCase {
                    id: "hero-1".to_string(),
                    query: "How does the retriever work?".to_string(),
                    expected_intent: Some("implementation".to_string()),
                    expected_files: vec!["retriever.rs".to_string()],
                    expected_identifiers: vec!["retrieve".to_string()],
                    expected_chunk_types: vec!["code".to_string()],
                    expected_projects: vec!["code-rag-chat".to_string()],
                    min_relevant_results: Some(3),
                    excluded_files: vec!["generator.rs".to_string()],
                    tags: vec!["hero".to_string(), "v1".to_string()],
                    notes: Some("V1 hero query".to_string()),
                },
                TestCase {
                    id: "overview-1".to_string(),
                    query: "What is code-rag-chat?".to_string(),
                    expected_intent: Some("overview".to_string()),
                    expected_files: vec![],
                    expected_identifiers: vec![],
                    expected_chunk_types: vec!["readme".to_string()],
                    expected_projects: vec![],
                    min_relevant_results: None,
                    excluded_files: vec![],
                    tags: vec!["overview".to_string()],
                    notes: None,
                },
            ],
        }
    }

    #[test]
    fn serde_round_trip() {
        let dataset = sample_dataset();
        let json = serde_json::to_string(&dataset).unwrap();
        let deserialized: TestDataset = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.cases.len(), dataset.cases.len());
        assert_eq!(deserialized.cases[0].id, "hero-1");
        assert_eq!(deserialized.description, "Test");
        assert_eq!(deserialized.schema_version, 1);
    }

    #[test]
    fn filter_by_tag_hero() {
        let dataset = sample_dataset();
        let heroes = dataset.filter_by_tag("hero");
        assert_eq!(heroes.len(), 1);
        assert_eq!(heroes[0].id, "hero-1");
    }

    #[test]
    fn filter_by_tag_nonexistent() {
        let dataset = sample_dataset();
        let empty = dataset.filter_by_tag("nonexistent");
        assert!(empty.is_empty());
    }

    #[test]
    fn validate_good_dataset() {
        let dataset = sample_dataset();
        let warnings = dataset.validate();
        assert!(warnings.is_empty(), "Unexpected warnings: {:?}", warnings);
    }

    #[test]
    fn validate_empty_id() {
        let dataset = TestDataset {
            description: "Test".to_string(),
            schema_version: 1,
            cases: vec![minimal_case("", "some query")],
        };
        let warnings = dataset.validate();
        assert!(warnings.iter().any(|w| w.contains("empty id")));
    }

    #[test]
    fn validate_duplicate_ids() {
        let dataset = TestDataset {
            description: "Test".to_string(),
            schema_version: 1,
            cases: vec![
                minimal_case("dup", "query 1"),
                minimal_case("dup", "query 2"),
            ],
        };
        let warnings = dataset.validate();
        assert!(warnings.iter().any(|w| w.contains("Duplicate")));
    }

    #[test]
    fn validate_unknown_intent() {
        let mut case = minimal_case("test-1", "query");
        case.expected_intent = Some("nonsense".to_string());
        let dataset = TestDataset {
            description: "Test".to_string(),
            schema_version: 1,
            cases: vec![case],
        };
        let warnings = dataset.validate();
        assert!(warnings.iter().any(|w| w.contains("Unknown intent")));
    }

    #[test]
    fn validate_zero_cases() {
        let dataset = TestDataset {
            description: "Test".to_string(),
            schema_version: 1,
            cases: vec![],
        };
        let warnings = dataset.validate();
        assert!(warnings.iter().any(|w| w.contains("zero")));
    }

    #[test]
    fn minimal_case_defaults() {
        let json = r#"{"id": "min", "query": "test query"}"#;
        let case: TestCase = serde_json::from_str(json).unwrap();
        assert_eq!(case.id, "min");
        assert!(case.expected_intent.is_none());
        assert!(case.expected_files.is_empty());
        assert!(case.expected_identifiers.is_empty());
        assert!(case.expected_chunk_types.is_empty());
        assert!(case.expected_projects.is_empty());
        assert!(case.min_relevant_results.is_none());
        assert!(case.excluded_files.is_empty());
        assert!(case.tags.is_empty());
        assert!(case.notes.is_none());
    }

    #[test]
    fn unknown_json_fields_ignored() {
        let json = r#"{"id": "test", "query": "q", "future_field": 42, "another": "hello"}"#;
        let case: TestCase = serde_json::from_str(json).unwrap();
        assert_eq!(case.id, "test");
    }

    #[test]
    fn min_relevant_results_round_trip() {
        let mut case = minimal_case("test", "query");
        case.min_relevant_results = Some(3);
        let json = serde_json::to_string(&case).unwrap();
        let deserialized: TestCase = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.min_relevant_results, Some(3));
    }

    #[test]
    fn min_relevant_results_absent() {
        let json = r#"{"id": "test", "query": "q"}"#;
        let case: TestCase = serde_json::from_str(json).unwrap();
        assert!(case.min_relevant_results.is_none());
    }

    #[test]
    fn excluded_files_round_trip() {
        let mut case = minimal_case("test", "query");
        case.excluded_files = vec!["retriever.rs".to_string()];
        let json = serde_json::to_string(&case).unwrap();
        let deserialized: TestCase = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.excluded_files, vec!["retriever.rs"]);
    }

    #[test]
    fn excluded_files_absent() {
        let json = r#"{"id": "test", "query": "q"}"#;
        let case: TestCase = serde_json::from_str(json).unwrap();
        assert!(case.excluded_files.is_empty());
    }

    #[test]
    fn validate_strict_good_dataset() {
        let dataset = sample_dataset();
        assert!(dataset.validate_strict().is_ok());
    }

    #[test]
    fn validate_strict_rejects_bad_dataset() {
        let dataset = TestDataset {
            description: "Test".to_string(),
            schema_version: 1,
            cases: vec![], // zero cases triggers warning
        };
        assert!(dataset.validate_strict().is_err());
    }

    #[test]
    fn smoke_case_soft_expectations_only() {
        let json = r#"{
            "id": "smoke-1",
            "query": "Give me an overview",
            "min_relevant_results": 3,
            "excluded_files": ["generator.rs"],
            "tags": ["smoke"]
        }"#;
        let case: TestCase = serde_json::from_str(json).unwrap();
        assert_eq!(case.min_relevant_results, Some(3));
        assert_eq!(case.excluded_files, vec!["generator.rs"]);
        assert!(case.expected_files.is_empty());
        assert!(case.expected_identifiers.is_empty());
    }
}
