use clap::Parser;

use code_rag_chat::engine::intent::IntentClassifier;
use code_rag_chat::engine::{DualEmbeddingConfig, EngineConfig, HybridConfig, RerankConfig};
use code_rag_chat::harness::dataset::TestDataset;
use code_rag_chat::harness::metrics;
use code_rag_chat::harness::report::{self, HarnessReport, SystemConfig};
use code_rag_chat::harness::runner;
use code_rag_chat::store::{Embedder, Reranker, VectorStore};

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

    /// Report label for identification (e.g. "baseline", "post_a1", "post_a1_b1")
    #[arg(long, default_value = "baseline")]
    label: String,

    /// Tracks completed at time of measurement (repeatable, e.g. --track a1 --track b1)
    #[arg(long = "track")]
    completed_tracks: Vec<String>,

    /// Enable cross-encoder reranking (auto-downloads ms-marco-MiniLM-L-6-v2)
    #[arg(long)]
    rerank: bool,

    /// Enable hybrid (BM25 + semantic) search
    #[arg(long)]
    hybrid: bool,

    /// B5: Enable dual-embedding retrieval (body_vector + signature_vector arms)
    #[arg(long = "dual-embedding")]
    dual_embedding: bool,

    /// Over-retrieval multiplier for code chunks
    #[arg(long, default_value = "4")]
    code_fetch_multiplier: usize,
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
    let mut classifier = IntentClassifier::build(|texts: &[&str]| embedder.embed_batch(texts))?;
    if let Ok(Ok(v)) = std::env::var("INTENT_THRESHOLD").map(|s| s.parse::<f32>()) {
        println!("Overriding intent threshold: {}", v);
        classifier = classifier.with_threshold(v);
    }
    if let Ok(Ok(v)) = std::env::var("INTENT_MARGIN").map(|s| s.parse::<f32>()) {
        println!("Overriding intent margin: {}", v);
        classifier = classifier.with_margin_threshold(v);
    }
    if let Ok(Ok(v)) = std::env::var("INTENT_KNN_K").map(|s| s.parse::<usize>()) {
        println!("Enabling k-NN voting with k={}", v);
        classifier = classifier.with_knn_k(Some(v));
    }
    let store = VectorStore::new(&cli.db_path, embedder.dimension()).await?;

    let mut config = EngineConfig::default();

    // Initialize reranker if enabled (auto-downloads model on first use)
    let mut reranker = if cli.rerank {
        println!("Initializing reranker (ms-marco-MiniLM-L-6-v2)...");
        let r = Reranker::new()?;
        config.rerank = RerankConfig {
            enabled: true,
            code_fetch_multiplier: cli.code_fetch_multiplier,
            ..Default::default()
        };
        Some(r)
    } else {
        None
    };

    // HybridConfig::default() has enabled=true, so the flag must SET the value
    // (not guard it) to get a real h0 vs h1 contrast during sweeps.
    config.hybrid = HybridConfig {
        enabled: cli.hybrid,
        ..Default::default()
    };

    config.dual_embedding = DualEmbeddingConfig {
        enabled: cli.dual_embedding,
    };

    // Warmup: force model load before measurement loop
    let _ = embedder.embed_one("warmup");

    // 4. Run all queries
    let owned_cases: Vec<_> = cases.into_iter().cloned().collect();
    let query_results = runner::run_all(
        &owned_cases,
        &mut embedder,
        &classifier,
        reranker.as_mut(),
        &store,
        &config,
        cli.ground_truth_intent,
        cli.verbose,
    )
    .await?;

    // 5. Compute metrics — join by case_id (ground-truth mode skips cases without
    // expected_intent, so positional zip would mis-pair results with cases).
    let case_by_id: std::collections::HashMap<&str, &_> =
        owned_cases.iter().map(|c| (c.id.as_str(), c)).collect();
    let pairs: Vec<_> = query_results
        .iter()
        .filter_map(|r| case_by_id.get(r.case_id.as_str()).map(|c| (r.clone(), *c)))
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
            label: cli.label.clone(),
            completed_tracks: cli.completed_tracks.clone(),
            reranking_enabled: cli.rerank,
            reranker_model: if cli.rerank {
                Some("ms-marco-MiniLM-L-6-v2".to_string())
            } else {
                None
            },
            code_fetch_multiplier: if cli.rerank {
                Some(cli.code_fetch_multiplier)
            } else {
                None
            },
            hybrid_enabled: cli.hybrid,
            dual_embedding_enabled: cli.dual_embedding,
        },
        aggregate,
        generation_cost: None,
        by_intent,
        per_query,
        warnings,
    };

    // 9. Write output
    std::fs::create_dir_all(&cli.output)?;
    let json_path =
        std::path::Path::new(&cli.output).join(format!("{}_{}.json", cli.label, git_commit));
    let md_path =
        std::path::Path::new(&cli.output).join(format!("{}_{}.md", cli.label, git_commit));

    report::write_json(&harness_report, &json_path)?;
    report::write_markdown(&harness_report, &md_path)?;

    println!("Reports written to:");
    println!("  JSON: {}", json_path.display());
    println!("  Markdown: {}", md_path.display());

    // 10. Print summary
    report::print_summary(&harness_report);

    Ok(())
}
