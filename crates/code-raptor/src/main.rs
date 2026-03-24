//! code-raptor CLI - Code knowledge graph construction tool

use clap::{Parser, Subcommand};
use code_raptor::{
    DEFAULT_EMBEDDING_MODEL, DeletionsByTable, Embedder, ExistingFileIndex, IngestionResult,
    IngestionStats, VectorStore, format_code_for_embedding, format_crate_for_embedding,
    format_module_doc_for_embedding, format_readme_for_embedding, reconcile, run_ingestion,
};
use code_rag_types::{CodeChunk, CrateChunk, ModuleDocChunk, ReadmeChunk};
use std::collections::{HashMap, HashSet};
use tracing::info;

/// Batch size for embedding processing to reduce peak memory usage
const EMBEDDING_BATCH_SIZE: usize = 25;

/// LanceDB table names (must match coderag-store conventions)
const CODE_TABLE: &str = "code_chunks";
const README_TABLE: &str = "readme_chunks";
const CRATE_TABLE: &str = "crate_chunks";
const MODULE_DOC_TABLE: &str = "module_doc_chunks";

#[derive(Parser)]
#[command(name = "code-raptor")]
#[command(about = "Build code knowledge graphs for RAG applications")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Ingest a repository and build the knowledge graph
    Ingest {
        /// Path to the repository to ingest
        #[arg(value_name = "PATH")]
        repo_path: String,

        /// Path to the LanceDB database
        #[arg(short, long, default_value = "data/portfolio.lance")]
        db_path: String,

        /// Explicit project name (defaults to repo directory name)
        #[arg(short, long)]
        project_name: Option<String>,

        /// Force full re-index (default: incremental)
        #[arg(long, conflicts_with = "dry_run")]
        full: bool,

        /// Show what would change without modifying DB
        #[arg(long)]
        dry_run: bool,
    },
    /// Show status of indexed repositories
    Status {
        /// Path to the LanceDB database
        #[arg(short, long, default_value = "data/portfolio.lance")]
        db_path: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into())
                .add_directive("lance::file_audit=warn".parse().unwrap()),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Ingest {
            repo_path,
            db_path,
            project_name,
            full,
            dry_run,
        } => {
            info!("Ingesting repository: {}", repo_path);
            info!("Database path: {}", db_path);
            if let Some(ref name) = project_name {
                info!("Project name override: {}", name);
            }

            // Step 1: Parse code into chunks (sync, no DB)
            let (result, calls_map) = run_ingestion(&repo_path, project_name.as_deref());
            info!(
                "Parsed: {} code, {} readme, {} crate, {} module_doc chunks",
                result.code_chunks.len(),
                result.readme_chunks.len(),
                result.crate_chunks.len(),
                result.module_doc_chunks.len()
            );

            // Step 2: Initialize embedder + store
            let mut embedder = Embedder::new()?;
            let dimension = embedder.dimension();
            let store = VectorStore::new(&db_path, dimension).await?;

            // Step 3: Run the appropriate ingestion mode
            if full {
                run_full_ingestion(&result, &store, &mut embedder, &calls_map).await?;
            } else {
                run_incremental_ingestion(&result, &store, &mut embedder, dry_run, &calls_map)
                    .await?;
            }
        }
        Commands::Status { db_path } => {
            info!("Checking status of: {}", db_path);
            let store = VectorStore::new(&db_path, 384).await?;
            let projects = store.list_projects().await?;
            info!("Projects indexed: {:?}", projects);
        }
    }

    Ok(())
}

// ============================================================================
// Orchestration: Full ingestion
// ============================================================================

async fn run_full_ingestion(
    result: &IngestionResult,
    store: &VectorStore,
    embedder: &mut Embedder,
    calls_map: &HashMap<String, Vec<String>>,
) -> anyhow::Result<()> {
    info!("Mode: full re-index");

    let projects = collect_project_names(result);

    // Delete all existing chunks for these projects
    for project in &projects {
        delete_project_from_all_tables(store, project).await?;
    }

    // Embed and store all chunks
    embed_and_store_all(result, store, embedder, calls_map).await?;

    info!(
        "Full ingestion complete: {} code, {} readme, {} crate, {} module_doc chunks",
        result.code_chunks.len(),
        result.readme_chunks.len(),
        result.crate_chunks.len(),
        result.module_doc_chunks.len()
    );

    Ok(())
}

// ============================================================================
// Orchestration: Incremental ingestion
// ============================================================================

async fn run_incremental_ingestion(
    result: &IngestionResult,
    store: &VectorStore,
    embedder: &mut Embedder,
    dry_run: bool,
    calls_map: &HashMap<String, Vec<String>>,
) -> anyhow::Result<()> {
    info!("Mode: {}", if dry_run { "dry run" } else { "incremental" });

    let projects = collect_project_names(result);

    // Verify embedding model compatibility before diffing
    check_embedding_model_version(store, &projects).await?;

    // Build existing index from DB (async I/O)
    let existing = build_existing_index(store, &projects).await?;

    // Reconcile: pure data comparison (sync, no DB)
    let diff = reconcile(result, &existing);
    log_reconcile_stats(&diff.stats);

    if dry_run {
        info!("Dry run complete — no changes applied");
        return Ok(());
    }

    if diff.stats.chunks_to_insert == 0 && diff.stats.chunks_to_delete == 0 {
        info!("No changes detected — database is up to date");
        return Ok(());
    }

    // Insert new chunks first (safer on crash: duplicates > missing data)
    embed_and_store_all(&diff.to_insert, store, embedder, calls_map).await?;

    // Then delete old chunks
    apply_deletions(store, &diff.to_delete).await?;

    info!("Incremental ingestion complete");
    Ok(())
}

// ============================================================================
// Helpers: Index building + model check
// ============================================================================

fn collect_project_names(result: &IngestionResult) -> HashSet<String> {
    result
        .code_chunks
        .iter()
        .map(|c| &c.project_name)
        .chain(result.readme_chunks.iter().map(|c| &c.project_name))
        .chain(result.crate_chunks.iter().map(|c| &c.project_name))
        .chain(result.module_doc_chunks.iter().map(|c| &c.project_name))
        .cloned()
        .collect()
}

async fn check_embedding_model_version(
    store: &VectorStore,
    projects: &HashSet<String>,
) -> anyhow::Result<()> {
    for project in projects {
        if let Some(stored) = store.get_embedding_model_version(project).await?
            && stored != DEFAULT_EMBEDDING_MODEL
        {
            anyhow::bail!(
                "Embedding model mismatch for project '{}': stored='{}', current='{}'. \
                 Use --full to force re-index.",
                project,
                stored,
                DEFAULT_EMBEDDING_MODEL
            );
        }
    }
    Ok(())
}

async fn build_existing_index(
    store: &VectorStore,
    projects: &HashSet<String>,
) -> anyhow::Result<ExistingFileIndex> {
    let mut index = ExistingFileIndex::default();

    for project in projects {
        // Code + README: file_path → (content_hash, Vec<chunk_id>)
        index.code_files.extend(
            store
                .get_file_index(CODE_TABLE, project, "file_path")
                .await?,
        );
        index.readme_files.extend(
            store
                .get_file_index(README_TABLE, project, "file_path")
                .await?,
        );

        // Crate chunks: crate_name → (content_hash, single chunk_id)
        for (name, (hash, ids)) in store
            .get_file_index(CRATE_TABLE, project, "crate_name")
            .await?
        {
            if let Some(id) = ids.into_iter().next() {
                index.crate_entries.insert(name, (hash, id));
            }
        }

        // Module doc chunks: file_path → (content_hash, single chunk_id)
        for (path, (hash, ids)) in store
            .get_file_index(MODULE_DOC_TABLE, project, "file_path")
            .await?
        {
            if let Some(id) = ids.into_iter().next() {
                index.module_doc_files.insert(path, (hash, id));
            }
        }
    }

    Ok(index)
}

// ============================================================================
// Helpers: Apply changes to store
// ============================================================================

async fn delete_project_from_all_tables(
    store: &VectorStore,
    project_name: &str,
) -> anyhow::Result<()> {
    info!("Deleting all chunks for project: {}", project_name);
    for table in [CODE_TABLE, README_TABLE, CRATE_TABLE, MODULE_DOC_TABLE] {
        store.delete_chunks_by_project(table, project_name).await?;
    }
    Ok(())
}

async fn apply_deletions(store: &VectorStore, deletions: &DeletionsByTable) -> anyhow::Result<()> {
    if !deletions.code_chunk_ids.is_empty() {
        store
            .delete_chunks_by_ids(CODE_TABLE, &deletions.code_chunk_ids)
            .await?;
        info!("Deleted {} code chunks", deletions.code_chunk_ids.len());
    }
    if !deletions.readme_chunk_ids.is_empty() {
        store
            .delete_chunks_by_ids(README_TABLE, &deletions.readme_chunk_ids)
            .await?;
        info!("Deleted {} readme chunks", deletions.readme_chunk_ids.len());
    }
    if !deletions.crate_chunk_ids.is_empty() {
        store
            .delete_chunks_by_ids(CRATE_TABLE, &deletions.crate_chunk_ids)
            .await?;
        info!("Deleted {} crate chunks", deletions.crate_chunk_ids.len());
    }
    if !deletions.module_doc_chunk_ids.is_empty() {
        store
            .delete_chunks_by_ids(MODULE_DOC_TABLE, &deletions.module_doc_chunk_ids)
            .await?;
        info!(
            "Deleted {} module doc chunks",
            deletions.module_doc_chunk_ids.len()
        );
    }
    Ok(())
}

async fn embed_and_store_all(
    result: &IngestionResult,
    store: &VectorStore,
    embedder: &mut Embedder,
    calls_map: &HashMap<String, Vec<String>>,
) -> anyhow::Result<()> {
    let code_count = embed_and_store_code(&result.code_chunks, store, embedder, calls_map).await?;
    if code_count > 0 {
        info!("Stored {} code chunks", code_count);
    }

    let readme_count = embed_and_store_readme(&result.readme_chunks, store, embedder).await?;
    if readme_count > 0 {
        info!("Stored {} readme chunks", readme_count);
    }

    let crate_count = embed_and_store_crates(&result.crate_chunks, store, embedder).await?;
    if crate_count > 0 {
        info!("Stored {} crate chunks", crate_count);
    }

    let module_doc_count =
        embed_and_store_module_docs(&result.module_doc_chunks, store, embedder).await?;
    if module_doc_count > 0 {
        info!("Stored {} module doc chunks", module_doc_count);
    }

    Ok(())
}

fn log_reconcile_stats(stats: &IngestionStats) {
    info!(
        unchanged = stats.files_unchanged,
        changed = stats.files_changed,
        new = stats.files_new,
        deleted = stats.files_deleted,
        to_insert = stats.chunks_to_insert,
        to_delete = stats.chunks_to_delete,
        "Reconcile summary"
    );
}

// ============================================================================
// Embedding helpers (one per chunk type, batched for memory efficiency)
// ============================================================================

async fn embed_and_store_code(
    chunks: &[CodeChunk],
    store: &VectorStore,
    embedder: &mut Embedder,
    calls_map: &HashMap<String, Vec<String>>,
) -> anyhow::Result<usize> {
    if chunks.is_empty() {
        return Ok(0);
    }

    info!("Embedding {} code chunks...", chunks.len());
    let empty = Vec::new();
    let mut total = 0;
    for batch in chunks.chunks(EMBEDDING_BATCH_SIZE) {
        let texts: Vec<String> = batch
            .iter()
            .map(|c| {
                let chunk_calls = calls_map.get(&c.chunk_id).unwrap_or(&empty);
                format_code_for_embedding(
                    &c.identifier,
                    &c.language,
                    c.docstring.as_deref(),
                    &c.code_content,
                    chunk_calls,
                )
            })
            .collect();
        let text_refs: Vec<&str> = texts.iter().map(|s| s.as_str()).collect();
        let embeddings = embedder.embed_batch(&text_refs)?;
        total += store.upsert_code_chunks(batch, embeddings).await?;
    }
    Ok(total)
}

async fn embed_and_store_readme(
    chunks: &[ReadmeChunk],
    store: &VectorStore,
    embedder: &mut Embedder,
) -> anyhow::Result<usize> {
    if chunks.is_empty() {
        return Ok(0);
    }

    info!("Embedding {} readme chunks...", chunks.len());
    let mut total = 0;
    for batch in chunks.chunks(EMBEDDING_BATCH_SIZE) {
        let texts: Vec<String> = batch
            .iter()
            .map(|c| format_readme_for_embedding(&c.project_name, &c.content))
            .collect();
        let text_refs: Vec<&str> = texts.iter().map(|s| s.as_str()).collect();
        let embeddings = embedder.embed_batch(&text_refs)?;
        total += store.upsert_readme_chunks(batch, embeddings).await?;
    }
    Ok(total)
}

async fn embed_and_store_crates(
    chunks: &[CrateChunk],
    store: &VectorStore,
    embedder: &mut Embedder,
) -> anyhow::Result<usize> {
    if chunks.is_empty() {
        return Ok(0);
    }

    info!("Embedding {} crate chunks...", chunks.len());
    let mut total = 0;
    for batch in chunks.chunks(EMBEDDING_BATCH_SIZE) {
        let texts: Vec<String> = batch
            .iter()
            .map(|c| {
                format_crate_for_embedding(&c.crate_name, c.description.as_deref(), &c.dependencies)
            })
            .collect();
        let text_refs: Vec<&str> = texts.iter().map(|s| s.as_str()).collect();
        let embeddings = embedder.embed_batch(&text_refs)?;
        total += store.upsert_crate_chunks(batch, embeddings).await?;
    }
    Ok(total)
}

async fn embed_and_store_module_docs(
    chunks: &[ModuleDocChunk],
    store: &VectorStore,
    embedder: &mut Embedder,
) -> anyhow::Result<usize> {
    if chunks.is_empty() {
        return Ok(0);
    }

    info!("Embedding {} module doc chunks...", chunks.len());
    let mut total = 0;
    for batch in chunks.chunks(EMBEDDING_BATCH_SIZE) {
        let texts: Vec<String> = batch
            .iter()
            .map(|c| format_module_doc_for_embedding(&c.module_name, &c.doc_content))
            .collect();
        let text_refs: Vec<&str> = texts.iter().map(|s| s.as_str()).collect();
        let embeddings = embedder.embed_batch(&text_refs)?;
        total += store.upsert_module_doc_chunks(batch, embeddings).await?;
    }
    Ok(total)
}
