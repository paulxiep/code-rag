use std::fmt::Write as FmtWrite;
use std::path::Path;

use serde::Serialize;

use super::dataset::TestCase;
use super::matching::evaluate_hits;
use super::metrics::{self, AggregateMetrics, IntentMetrics};
use super::runner::QueryResult;

#[derive(Debug, Serialize)]
pub struct HarnessReport {
    pub timestamp: String,
    pub git_commit: String,
    pub system: SystemConfig,
    pub aggregate: AggregateMetrics,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generation_cost: Option<GenerationCostMetrics>,
    pub by_intent: Vec<IntentMetrics>,
    pub per_query: Vec<QueryReport>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct GenerationCostMetrics {
    pub tokens_per_query_p50: u64,
    pub tokens_per_query_p95: u64,
}

#[derive(Debug, Serialize)]
pub struct SystemConfig {
    pub embedding_model: String,
    pub db_path: String,
    pub dataset_path: String,
    pub total_cases: usize,
    pub use_classifier: bool,
    /// Run label for report identification, e.g. "baseline", "post_a1", "post_a1_b1"
    pub label: String,
    /// Tracks completed at time of measurement, e.g. [] for baseline, ["a1", "b1"] for post-track
    pub completed_tracks: Vec<String>,
    /// Whether cross-encoder reranking was enabled
    pub reranking_enabled: bool,
    /// Reranker model name, if reranking enabled
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reranker_model: Option<String>,
    /// Code over-retrieval multiplier, if reranking enabled
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code_fetch_multiplier: Option<usize>,
    /// Whether hybrid (BM25 + semantic) search was enabled
    pub hybrid_enabled: bool,
}

#[derive(Debug, Serialize)]
pub struct QueryReport {
    pub case_id: String,
    pub query: String,
    pub expected_intent: Option<String>,
    pub classified_intent: String,
    pub intent_correct: Option<bool>,
    pub intent_confidence: f32,
    pub intent_margin: f32,
    pub recall_at_5: f32,
    pub recall_at_10: f32,
    pub mrr: f32,
    pub latency_ms: u64,
    pub top_results: Vec<String>,
    pub file_hits: Vec<String>,
    pub file_misses: Vec<String>,
    pub identifier_hits: Vec<String>,
    pub identifier_misses: Vec<String>,
    pub project_hits: Vec<String>,
    pub project_misses: Vec<String>,
    pub excluded_violations: Vec<String>,
    pub min_relevant_met: Option<bool>,
    pub relevant_count: usize,
}

/// Build per-query reports from results and test cases.
pub fn build_query_reports(results: &[(QueryResult, &TestCase)]) -> Vec<QueryReport> {
    results
        .iter()
        .map(|(result, case)| {
            let hits = evaluate_hits(result, case, 10);
            let top_results: Vec<String> = result
                .retrieved
                .iter()
                .take(10)
                .map(|item| {
                    format!(
                        "{}:{} ({}%)",
                        item.flat.chunk_type,
                        item.flat
                            .identifier
                            .as_deref()
                            .unwrap_or(&item.flat.file_path),
                        (item.flat.relevance * 100.0).round() as u8
                    )
                })
                .collect();

            QueryReport {
                case_id: result.case_id.clone(),
                query: case.query.clone(),
                expected_intent: case.expected_intent.clone(),
                classified_intent: format!("{:?}", result.classified_intent).to_lowercase(),
                intent_correct: hits.intent_correct,
                intent_confidence: result.confidence,
                intent_margin: result.margin,
                recall_at_5: metrics::recall_at_k(result, case, 5),
                recall_at_10: metrics::recall_at_k(result, case, 10),
                mrr: metrics::mrr(result, case),
                latency_ms: result.latency.as_millis() as u64,
                top_results,
                file_hits: hits.file_hits,
                file_misses: hits.file_misses,
                identifier_hits: hits.identifier_hits,
                identifier_misses: hits.identifier_misses,
                project_hits: hits.project_hits,
                project_misses: hits.project_misses,
                excluded_violations: hits.excluded_file_violations,
                min_relevant_met: hits.min_relevant_met,
                relevant_count: hits.relevant_count,
            }
        })
        .collect()
}

/// Generate post-run warnings.
pub fn generate_warnings(results: &[(QueryResult, &TestCase)]) -> Vec<String> {
    let mut warnings = Vec::new();

    // Expected files never found in ANY query's results
    let mut expected_files: std::collections::HashMap<&str, usize> =
        std::collections::HashMap::new();
    for (_, case) in results {
        for f in &case.expected_files {
            expected_files.entry(f.as_str()).or_insert(0);
        }
    }
    for (result, case) in results {
        for f in &case.expected_files {
            if result
                .retrieved
                .iter()
                .any(|item| item.flat.file_path.contains(f.as_str()))
            {
                *expected_files.get_mut(f.as_str()).unwrap() += 1;
            }
        }
    }
    for (file, count) in &expected_files {
        if *count == 0 {
            warnings.push(format!(
                "Expected file '{}' referenced in test cases but never found in any results",
                file
            ));
        }
    }

    // All queries in a category have recall = 0
    let mut intent_recall: std::collections::HashMap<String, (usize, usize)> =
        std::collections::HashMap::new();
    for (result, case) in results {
        if let Some(intent) = &case.expected_intent {
            let has_expectations =
                !case.expected_files.is_empty() || !case.expected_identifiers.is_empty();
            if has_expectations {
                let entry = intent_recall.entry(intent.clone()).or_insert((0, 0));
                entry.0 += 1; // total
                if metrics::recall_at_k(result, case, 5) == 0.0 {
                    entry.1 += 1; // zero recall
                }
            }
        }
    }
    for (intent, (total, zeros)) in &intent_recall {
        if *zeros == *total && *total > 0 {
            warnings.push(format!(
                "All {} queries with intent '{}' have recall@5 = 0 — possible systemic issue",
                total, intent
            ));
        }
    }

    // Latency outliers > 5x median
    let mut latencies: Vec<u64> = results
        .iter()
        .map(|(r, _)| r.latency.as_millis() as u64)
        .collect();
    latencies.sort();
    if latencies.len() >= 3 {
        let median = latencies[latencies.len() / 2];
        let threshold = median * 5;
        for (result, _) in results {
            let ms = result.latency.as_millis() as u64;
            if ms > threshold && threshold > 0 {
                warnings.push(format!(
                    "Query '{}' latency {}ms > 5x median {}ms — possible cold-start or resource issue",
                    result.case_id, ms, median
                ));
            }
        }
    }

    warnings
}

/// Write full report as JSON.
pub fn write_json(report: &HarnessReport, path: &Path) -> anyhow::Result<()> {
    let json = serde_json::to_string_pretty(report)?;
    std::fs::write(path, json)?;
    Ok(())
}

/// Write human-readable Markdown summary.
pub fn write_markdown(report: &HarnessReport, path: &Path) -> anyhow::Result<()> {
    let mut md = String::new();

    writeln!(md, "# Quality Harness Report\n")?;
    writeln!(
        md,
        "**Label:** {} | **Date:** {} | **Commit:** {} | **Model:** {}",
        report.system.label, report.timestamp, report.git_commit, report.system.embedding_model
    )?;
    if !report.system.completed_tracks.is_empty() {
        writeln!(
            md,
            "**Completed tracks:** {}",
            report.system.completed_tracks.join(", ")
        )?;
    }
    if report.system.reranking_enabled {
        writeln!(
            md,
            "**Reranking:** {} (code {}x)",
            report.system.reranker_model.as_deref().unwrap_or("unknown"),
            report.system.code_fetch_multiplier.unwrap_or(4)
        )?;
    }
    if report.system.hybrid_enabled {
        writeln!(md, "**Hybrid search:** BM25 + semantic (RRF fusion)")?;
    }
    writeln!(
        md,
        "**Dataset:** {} ({} queries)\n",
        report.system.dataset_path, report.system.total_cases
    )?;

    // Aggregate table
    writeln!(md, "## Aggregate\n")?;
    writeln!(md, "| Metric | Value |")?;
    writeln!(md, "|--------|-------|")?;
    writeln!(md, "| recall@5 | {:.2} |", report.aggregate.recall_at_5)?;
    writeln!(md, "| recall@10 | {:.2} |", report.aggregate.recall_at_10)?;
    writeln!(md, "| MRR | {:.2} |", report.aggregate.mrr)?;
    writeln!(
        md,
        "| Intent accuracy | {:.0}% |",
        report.aggregate.intent_accuracy * 100.0
    )?;
    writeln!(
        md,
        "| Latency p50 | {}ms |",
        report.aggregate.latency_p50_ms
    )?;
    writeln!(
        md,
        "| Latency p95 | {}ms |",
        report.aggregate.latency_p95_ms
    )?;
    writeln!(
        md,
        "| Recall-scored queries | {} |",
        report.aggregate.recall_scored_queries
    )?;
    writeln!(
        md,
        "| No-expectation queries | {} |\n",
        report.aggregate.no_expectation_queries
    )?;

    // By Intent table
    if !report.by_intent.is_empty() {
        writeln!(md, "## By Intent\n")?;
        writeln!(
            md,
            "| Intent | Queries | recall@5 | recall@10 | Intent Acc |"
        )?;
        writeln!(
            md,
            "|--------|---------|----------|-----------|------------|"
        )?;
        for im in &report.by_intent {
            writeln!(
                md,
                "| {} | {} | {:.2} | {:.2} | {:.0}% |",
                im.intent,
                im.query_count,
                im.recall_at_5,
                im.recall_at_10,
                im.intent_accuracy * 100.0
            )?;
        }
        writeln!(md)?;
    }

    // Failures (recall@5 = 0) — only for cases with expectations
    let failures: Vec<&QueryReport> = report
        .per_query
        .iter()
        .filter(|q| {
            q.recall_at_5 == 0.0 && (!q.file_misses.is_empty() || !q.identifier_misses.is_empty())
        })
        .collect();
    if !failures.is_empty() {
        writeln!(md, "## Failures (recall@5 = 0)\n")?;
        writeln!(md, "| ID | Query | Expected | Got |")?;
        writeln!(md, "|----|-------|----------|-----|")?;
        for f in &failures {
            let expected: Vec<String> = f
                .file_misses
                .iter()
                .chain(f.identifier_misses.iter())
                .cloned()
                .collect();
            let got = if f.top_results.is_empty() {
                "(none)".to_string()
            } else {
                f.top_results[..f.top_results.len().min(3)].join(", ")
            };
            writeln!(
                md,
                "| {} | \"{}\" | {} | {} |",
                f.case_id,
                f.query,
                expected.join(", "),
                got
            )?;
        }
        writeln!(md)?;
    }

    // Exclusion Violations
    let violations: Vec<&QueryReport> = report
        .per_query
        .iter()
        .filter(|q| !q.excluded_violations.is_empty())
        .collect();
    if !violations.is_empty() {
        writeln!(md, "## Exclusion Violations\n")?;
        writeln!(md, "| ID | Query | Excluded File | Matched Item |")?;
        writeln!(md, "|----|-------|---------------|--------------|")?;
        for v in &violations {
            for exc in &v.excluded_violations {
                let matched = v
                    .top_results
                    .iter()
                    .find(|r| r.contains(exc))
                    .cloned()
                    .unwrap_or_else(|| "(in results)".to_string());
                writeln!(
                    md,
                    "| {} | \"{}\" | {} | {} |",
                    v.case_id, v.query, exc, matched
                )?;
            }
        }
        writeln!(md)?;
    }

    // Min Relevant Failures
    let min_failures: Vec<&QueryReport> = report
        .per_query
        .iter()
        .filter(|q| q.min_relevant_met == Some(false))
        .collect();
    if !min_failures.is_empty() {
        writeln!(md, "## Min Relevant Failures\n")?;
        writeln!(md, "| ID | Query | Required | Found |")?;
        writeln!(md, "|----|-------|----------|-------|")?;
        for mf in &min_failures {
            writeln!(
                md,
                "| {} | \"{}\" | ? | {} |",
                mf.case_id, mf.query, mf.relevant_count
            )?;
        }
        writeln!(md)?;
    }

    // Warnings
    if !report.warnings.is_empty() {
        writeln!(md, "## Warnings\n")?;
        for w in &report.warnings {
            writeln!(md, "- {}", w)?;
        }
    }

    std::fs::write(path, md)?;
    Ok(())
}

/// Print concise summary to stdout.
pub fn print_summary(report: &HarnessReport) {
    println!("\n=== Quality Harness Report ===");
    println!("Commit: {} | Date: {}", report.git_commit, report.timestamp);
    println!(
        "Dataset: {} ({} queries)",
        report.system.dataset_path, report.system.total_cases
    );
    println!("---");
    println!("recall@5:  {:.2}", report.aggregate.recall_at_5);
    println!("recall@10: {:.2}", report.aggregate.recall_at_10);
    println!("MRR:       {:.2}", report.aggregate.mrr);
    println!(
        "Intent:    {:.0}%",
        report.aggregate.intent_accuracy * 100.0
    );
    println!(
        "Latency:   p50={}ms p95={}ms",
        report.aggregate.latency_p50_ms, report.aggregate.latency_p95_ms
    );
    if !report.warnings.is_empty() {
        println!("Warnings:  {}", report.warnings.len());
    }
    println!("===\n");
}

/// Get git short hash, falling back to "unknown".
pub fn git_short_hash() -> String {
    std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
            } else {
                None
            }
        })
        .unwrap_or_else(|| "unknown".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_git_short_hash_not_empty() {
        let hash = git_short_hash();
        assert!(!hash.is_empty());
        // In a git repo, should be a short hash (7+ chars) or "unknown"
    }

    #[test]
    fn test_write_json_round_trip() {
        let report = HarnessReport {
            timestamp: "2026-04-02T00:00:00Z".to_string(),
            git_commit: "abc1234".to_string(),
            system: SystemConfig {
                embedding_model: "BGESmallENV15_384".to_string(),
                db_path: "./data/portfolio.lance".to_string(),
                dataset_path: "data/test_queries.json".to_string(),
                total_cases: 2,
                use_classifier: true,
                label: "baseline".to_string(),
                completed_tracks: vec![],
                reranking_enabled: false,
                reranker_model: None,
                code_fetch_multiplier: None,
                hybrid_enabled: false,
            },
            aggregate: AggregateMetrics {
                total_queries: 2,
                recall_scored_queries: 2,
                no_expectation_queries: 0,
                recall_at_5: 0.75,
                recall_at_10: 0.85,
                mrr: 0.65,
                intent_accuracy: 0.9,
                latency_p50_ms: 45,
                latency_p95_ms: 120,
            },
            generation_cost: None,
            by_intent: vec![],
            per_query: vec![],
            warnings: vec![],
        };

        let dir = tempfile::tempdir().unwrap();
        let json_path = dir.path().join("test.json");
        write_json(&report, &json_path).unwrap();

        let content = std::fs::read_to_string(&json_path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed["git_commit"], "abc1234");
        assert_eq!(parsed["aggregate"]["recall_at_5"], 0.75);
        // generation_cost should be absent (skip_serializing_if)
        assert!(parsed.get("generation_cost").is_none());
    }

    #[test]
    fn test_write_markdown_renders() {
        let report = HarnessReport {
            timestamp: "2026-04-02T00:00:00Z".to_string(),
            git_commit: "abc1234".to_string(),
            system: SystemConfig {
                embedding_model: "BGESmallENV15_384".to_string(),
                db_path: "./data/portfolio.lance".to_string(),
                dataset_path: "data/test_queries.json".to_string(),
                total_cases: 2,
                use_classifier: true,
                label: "baseline".to_string(),
                completed_tracks: vec![],
                reranking_enabled: false,
                reranker_model: None,
                code_fetch_multiplier: None,
                hybrid_enabled: false,
            },
            aggregate: AggregateMetrics {
                total_queries: 2,
                recall_scored_queries: 2,
                no_expectation_queries: 0,
                recall_at_5: 0.72,
                recall_at_10: 0.85,
                mrr: 0.61,
                intent_accuracy: 0.89,
                latency_p50_ms: 45,
                latency_p95_ms: 120,
            },
            generation_cost: None,
            by_intent: vec![],
            per_query: vec![],
            warnings: vec!["Test warning".to_string()],
        };

        let dir = tempfile::tempdir().unwrap();
        let md_path = dir.path().join("test.md");
        write_markdown(&report, &md_path).unwrap();

        let content = std::fs::read_to_string(&md_path).unwrap();
        assert!(content.contains("# Quality Harness Report"));
        assert!(content.contains("abc1234"));
        assert!(content.contains("0.72"));
        assert!(content.contains("Test warning"));
    }
}
