use clap::Parser;

use code_rag_chat::engine::EngineConfig;
use code_rag_chat::engine::intent::IntentClassifier;
use code_rag_chat::harness::dataset::TestDataset;
use code_rag_chat::harness::metrics;
use code_rag_chat::harness::report::{self, HarnessReport, SystemConfig};
use code_rag_chat::harness::runner;
use code_rag_chat::store::{Embedder, VectorStore};

#[derive(Parser)]
#[command(
    name = "code-rag-harness",
    about = "Quality measurement harness for code-rag retrieval"
)]
struct Cli {
    /// Path to test_queries.json
    #[arg(long, default_value = "data/test_queries.json")]
    dataset: String,

    /// Path to LanceDB
    #[arg(long, default_value = "./data/portfolio.lance")]
    db_path: String,

    /// Output directory for reports
    #[arg(long, default_value = "data/reports/")]
    output: String,

    /// Use expected_intent for routing instead of classifier
    #[arg(long)]
    ground_truth_intent: bool,

    /// Fail run on dataset validation warnings
    #[arg(long)]
    strict: bool,

    /// Filter test cases by tag (repeatable)
    #[arg(long)]
    tag: Vec<String>,

    /// Print per-query results to stdout
    #[arg(long)]
    verbose: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // 1. Load + validate dataset (fail fast before embedder init)
    let dataset_path = std::path::Path::new(&cli.dataset);
    if !dataset_path.exists() {
        anyhow::bail!("Dataset file not found: {}", cli.dataset);
    }

    let dataset = TestDataset::load(dataset_path)?;

    if cli.strict {
        dataset.validate_strict()?;
    } else {
        let warnings = dataset.validate();
        for w in &warnings {
            eprintln!("Warning: {}", w);
        }
    }

    // 2. Apply tag filters
    let cases: Vec<_> = if cli.tag.is_empty() {
        dataset.cases.iter().collect()
    } else {
        dataset
            .cases
            .iter()
            .filter(|c| cli.tag.iter().any(|t| c.tags.contains(t)))
            .collect()
    };

    if cases.is_empty() {
        println!("No test cases match the given filters. Exiting.");
        return Ok(());
    }

    println!("Loaded {} test cases from {}", cases.len(), cli.dataset);

    // 3. Initialize engine (mirrors AppState::from_config, minus LlmClient and Mutex)
    let mut embedder = Embedder::new()?;
    let classifier = IntentClassifier::build(|texts: &[&str]| embedder.embed_batch(texts))?;
    let store = VectorStore::new(&cli.db_path, embedder.dimension()).await?;
    let config = EngineConfig::default();

    // Warmup: force model load before measurement loop
    let _ = embedder.embed_one("warmup");

    // 4. Run all queries
    let owned_cases: Vec<_> = cases.into_iter().cloned().collect();
    let query_results = runner::run_all(
        &owned_cases,
        &mut embedder,
        &classifier,
        &store,
        &config,
        cli.ground_truth_intent,
        cli.verbose,
    )
    .await?;

    // 5. Compute metrics
    let pairs: Vec<_> = query_results
        .iter()
        .zip(owned_cases.iter())
        .map(|(r, c)| (r.clone(), c))
        .collect();

    let aggregate = metrics::compute_aggregate(&pairs);
    let by_intent = metrics::compute_by_intent(&pairs);

    // 6. Generate warnings
    let warnings = report::generate_warnings(&pairs);

    // 7. Build per-query reports
    let per_query = report::build_query_reports(&pairs);

    // 8. Build report
    let git_commit = report::git_short_hash();
    let timestamp = chrono::Utc::now().to_rfc3339();

    let harness_report = HarnessReport {
        timestamp,
        git_commit: git_commit.clone(),
        system: SystemConfig {
            embedding_model: "BGESmallENV15_384".to_string(),
            db_path: cli.db_path.clone(),
            dataset_path: cli.dataset.clone(),
            total_cases: owned_cases.len(),
            use_classifier: !cli.ground_truth_intent,
        },
        aggregate,
        generation_cost: None,
        by_intent,
        per_query,
        warnings,
    };

    // 9. Write output
    std::fs::create_dir_all(&cli.output)?;
    let json_path = std::path::Path::new(&cli.output).join(format!("{}.json", git_commit));
    let md_path = std::path::Path::new(&cli.output).join(format!("{}.md", git_commit));

    report::write_json(&harness_report, &json_path)?;
    report::write_markdown(&harness_report, &md_path)?;

    println!("Reports written to:");
    println!("  JSON: {}", json_path.display());
    println!("  Markdown: {}", md_path.display());

    // 10. Print summary
    report::print_summary(&harness_report);

    Ok(())
}
