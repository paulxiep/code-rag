//! code-raptor CLI - Code knowledge graph construction tool
//!
//! Thin shell over the shared `code_raptor::orchestrate` lib so the same
//! ingest pipeline drives both this standalone CLI (used by the harness +
//! GitHub Pages export) and the `code-rag-mcp` `ingest` subcommand.

mod export;

use clap::{Parser, Subcommand};
use code_raptor::{IngestOpts, VectorStore, ingest_repo};
use tracing::info;

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

        /// Treat the target path as a single repo: all chunks share one
        /// project name (derived from repo dirname unless --project-name is
        /// set) instead of the multi-project "parent dir with sibling
        /// projects" default.
        #[arg(long)]
        single_repo: bool,

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
    /// Export all chunks with embeddings for static GitHub Pages demo
    Export {
        /// Path to the LanceDB database
        #[arg(short, long, default_value = "data/portfolio.lance")]
        db_path: String,

        /// Output JSON file path
        #[arg(short, long)]
        output: String,
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
            single_repo,
            full,
            dry_run,
        } => {
            ingest_repo(IngestOpts {
                repo_path,
                db_path,
                project_name,
                single_repo,
                full,
                dry_run,
            })
            .await?;
        }
        Commands::Status { db_path } => {
            info!("Checking status of: {}", db_path);
            let store = VectorStore::new(&db_path, 384).await?;
            let projects = store.list_projects().await?;
            info!("Projects indexed: {:?}", projects);
        }
        Commands::Export { db_path, output } => {
            info!("Exporting from {} to {}", db_path, output);
            export::run_export(&db_path, &output).await?;
            info!("Export complete: {}", output);
        }
    }

    Ok(())
}
