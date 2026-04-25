//! code-rag MCP server — exposes the retrieval brain as Claude-Code tools.
//!
//! Phase 1 spike: one tool (`code_rag_search`). Phase 2 adds `code_rag_graph`,
//! `code_rag_overview`, `code_rag_neighbors`, and `code_rag_reindex`.
//!
//! Process model: single-request-at-a-time. The embedder and reranker ONNX
//! sessions are already `Mutex`-guarded inside `AppState`; tool handlers
//! acquire those locks briefly and drop them before returning. That matches
//! the HTTP server's behaviour at `src/api/handlers.rs:42`.
//!
//! Protocol channel: stdout. All tracing output MUST go to stderr, or the
//! JSON-RPC stream corrupts and the Claude Code client disconnects.

use std::sync::Arc;

use anyhow::Result;
use clap::Parser;
use clap::Subcommand;
use code_rag_chat::{
    api::{AppState, build_sources},
    engine::{intent, retriever},
};
use code_rag_engine::{
    graph::{CallGraph, GraphDirection},
    intent::QueryIntent,
};
use rmcp::{
    ErrorData as McpError, ServerHandler, ServiceExt,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, Content, Implementation, ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router,
    transport::stdio,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tracing_subscriber::EnvFilter;

/// Skill text shipped at compile time so `init` doesn't depend on the source
/// tree being present at runtime.
const BUNDLED_SKILL: &str = include_str!("../skills/code-rag.md");

/// CLI for the MCP stdio binary.
///
/// Three modes, distinguished by argv:
/// - **Bare run** (no args, no subcommand) → reads `code-rag-mcp.config.yaml`
///   from the same directory as the exe and performs setup in `target_path`
///   (writes `.claude/skills/code-rag.md`, registers `code-rag` in
///   `.mcp.json`, appends `.code-rag-mcp/` to `.gitignore`). On first run with
///   no config file present, writes a default template and exits with
///   instructions. This is the "double-click the exe" install path.
/// - **Serve** (top-level flags like `--db-path`) — runs the stdio MCP server.
///   This is what Claude Code spawns via `.mcp.json`.
/// - **`ingest <path>`** subcommand — runs the same ingestion pipeline as the
///   internal `code-raptor` lib. Used by `code_rag_reindex` to spawn itself
///   recursively, so we ship one binary instead of two.
#[derive(Parser, Debug)]
#[command(name = "code-rag-mcp", about = "MCP server for code-rag retrieval")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Path to the LanceDB index produced by `code-raptor ingest ... --single-repo`.
    #[arg(long, default_value = "./.code-rag-mcp/index.lance", global = true)]
    db_path: String,

    /// Repo directory to re-ingest when `code_rag_reindex` is called. Default
    /// `.` matches the typical standalone workflow (run the server from the
    /// repo root you want to index).
    #[arg(long, default_value = ".", global = true)]
    repo_path: String,

    /// LLM model name — AppState requires one even though MCP never calls the
    /// LLM. Value is unused at tool-call time; any string is fine.
    #[arg(long, default_value = "gemini-2.5-flash", global = true)]
    model: String,

    /// Disable the cross-encoder reranker. Useful when running on machines
    /// without the ONNX model cached and offline.
    #[arg(long, global = true)]
    no_rerank: bool,

    /// Treat the repo as a multi-project workspace (parent folder containing
    /// sibling sub-projects) rather than a single repo. When set, the
    /// `code_rag_reindex` tool omits `--single-repo` from its self-spawned
    /// ingest call so each sibling subdirectory becomes its own project.
    /// Default (unset) is single-repo — the most common standalone workflow.
    #[arg(long, global = true)]
    workspace: bool,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Run the ingestion pipeline. Same surface as `code-raptor ingest`,
    /// kept here so the MCP server can spawn itself recursively for
    /// `code_rag_reindex` and we only ship one binary.
    Ingest {
        /// Path to the repository to ingest.
        #[arg(value_name = "PATH")]
        repo_path: String,

        /// Path to the LanceDB database.
        #[arg(long, default_value = "./.code-rag-mcp/index.lance")]
        db_path: String,

        /// Explicit project name (defaults to repo directory name when
        /// `--single-repo` is set; otherwise extracted per-subdir).
        #[arg(long)]
        project_name: Option<String>,

        /// Treat the target path as a single repo: all chunks share one
        /// project name (derived from repo dirname unless --project-name is
        /// set) instead of the multi-project "parent dir with sibling
        /// projects" default.
        #[arg(long)]
        single_repo: bool,

        /// Force full re-index (default: incremental).
        #[arg(long)]
        full: bool,
    },
}

/// Intent hint for `code_rag_search`. When absent, the classifier decides.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
enum IntentHint {
    Overview,
    Implementation,
    Relationship,
    Comparison,
}

impl From<IntentHint> for code_rag_engine::intent::QueryIntent {
    fn from(h: IntentHint) -> Self {
        use code_rag_engine::intent::QueryIntent;
        match h {
            IntentHint::Overview => QueryIntent::Overview,
            IntentHint::Implementation => QueryIntent::Implementation,
            IntentHint::Relationship => QueryIntent::Relationship,
            IntentHint::Comparison => QueryIntent::Comparison,
        }
    }
}

/// Parameters for `code_rag_search`.
#[derive(Debug, Deserialize, JsonSchema)]
struct SearchParams {
    /// Natural-language query to retrieve code/doc chunks for.
    query: String,
    /// Optional intent override. Skip classification when Claude has a strong
    /// prior (e.g. it knows the question is architectural/"overview").
    #[serde(default)]
    intent: Option<IntentHint>,
}

/// Serialized result shape returned to the MCP client. Subset of `SourceInfo`
/// with the fields Claude Code actually needs to act on a hit.
#[derive(Debug, Serialize)]
struct SearchHit {
    #[serde(rename = "type")]
    chunk_type: String,
    chunk_id: String,
    path: String,
    label: String,
    project: String,
    relevance: f32,
    line: usize,
}

/// Parameters for `code_rag_graph`.
#[derive(Debug, Deserialize, JsonSchema)]
struct GraphParams {
    /// Function / method identifier to look up (e.g. "retrieve", "handle_chat").
    identifier: String,
    /// Traversal direction. Defaults to "both".
    #[serde(default)]
    direction: Option<GraphDirectionHint>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
enum GraphDirectionHint {
    Callers,
    Callees,
    Both,
}

/// Parameters for `code_rag_overview`.
#[derive(Debug, Deserialize, JsonSchema)]
struct OverviewParams {
    /// Optional topic to focus on. When absent, returns a general overview.
    #[serde(default)]
    topic: Option<String>,
}

/// Parameters for `code_rag_neighbors`.
#[derive(Debug, Deserialize, JsonSchema)]
struct NeighborsParams {
    /// Chunk id (as returned by `code_rag_search`) whose surroundings to read.
    chunk_id: String,
    /// Line count around the chunk to return. Default 20.
    #[serde(default)]
    window: Option<usize>,
}

#[derive(Debug, Serialize)]
struct GraphEdgeView {
    chunk_id: String,
    identifier: String,
    file: String,
    /// Edge resolution confidence: 1 = same-file, 2 = import-based, 3 = unique-global.
    resolution_tier: u8,
}

#[derive(Clone)]
struct CodeRagServer {
    state: Arc<AppState>,
    repo_path: String,
    db_path: String,
    /// True when this MCP instance was launched with `--workspace`. Drives the
    /// `--single-repo` arg in the `code_rag_reindex` self-spawn.
    workspace_mode: bool,
    // Read by rmcp's tool_handler via trait dispatch; dead_code analysis misses it.
    #[allow(dead_code)]
    tool_router: ToolRouter<CodeRagServer>,
}

impl CodeRagServer {
    /// Shared search core used by both `code_rag_search` and `code_rag_overview`.
    /// Returns the `(intent, hits)` pair already shaped for JSON output.
    async fn run_search(
        &self,
        query: &str,
        forced_intent: Option<QueryIntent>,
    ) -> Result<(QueryIntent, Vec<SearchHit>), McpError> {
        let mut embedder_guard = self.state.embedder.lock().await;
        let query_embedding = embedder_guard
            .embed_one(query)
            .map_err(|e| McpError::internal_error(format!("embed failed: {e}"), None))?;

        let query_intent = if let Some(forced) = forced_intent {
            forced
        } else if let Some(pre) = intent::pre_classify_comparison(query) {
            pre
        } else {
            intent::classify(&query_embedding, &self.state.classifier).intent
        };
        let retrieval_config = intent::route(query_intent, &self.state.config.routing);

        let mut reranker_guard = match &self.state.reranker {
            Some(r) => Some(r.lock().await),
            None => None,
        };
        let result = retriever::retrieve(
            retriever::QueryContext {
                query,
                embedding: &query_embedding,
                intent: query_intent,
            },
            &self.state.store,
            &mut embedder_guard,
            &retrieval_config,
            &self.state.config,
            reranker_guard.as_deref_mut(),
        )
        .await
        .map_err(|e| McpError::internal_error(format!("retrieve failed: {e}"), None))?;
        drop(reranker_guard);
        drop(embedder_guard);

        let hits: Vec<SearchHit> = build_sources(&result)
            .into_iter()
            .map(|s| SearchHit {
                chunk_type: s.chunk_type,
                chunk_id: s.chunk_id,
                path: s.path,
                label: s.label,
                project: s.project,
                relevance: s.relevance,
                line: s.line,
            })
            .collect();

        Ok((result.intent, hits))
    }

    /// Load call edges across all projects in the index and build a CallGraph.
    /// For single-repo MCP deployments this is just one project; portfolio-style
    /// DBs get combined into one graph. chunk_ids are SHA-deterministic so
    /// combining across projects is safe.
    async fn load_call_graph(&self) -> Result<CallGraph, McpError> {
        let projects =
            self.state.store.list_projects().await.map_err(|e| {
                McpError::internal_error(format!("list_projects failed: {e}"), None)
            })?;

        let mut all_edges = Vec::new();
        for project in &projects {
            let edges = self.state.store.get_all_edges(project).await.map_err(|e| {
                McpError::internal_error(format!("get_all_edges failed: {e}"), None)
            })?;
            all_edges.extend(edges);
        }

        let id_pairs: Vec<(String, String)> = all_edges
            .iter()
            .flat_map(|e| {
                [
                    (e.caller_identifier.clone(), e.caller_chunk_id.clone()),
                    (e.callee_identifier.clone(), e.callee_chunk_id.clone()),
                ]
            })
            .collect();

        let mut graph = CallGraph::from_edges(
            all_edges
                .iter()
                .map(|e| (e.caller_chunk_id.clone(), e.callee_chunk_id.clone())),
        );
        graph.register_identifiers(id_pairs);
        Ok(graph)
    }
}

#[tool_router]
impl CodeRagServer {
    fn new(state: Arc<AppState>, repo_path: String, db_path: String, workspace_mode: bool) -> Self {
        Self {
            state,
            repo_path,
            db_path,
            workspace_mode,
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        description = "Semantic + hybrid search over the indexed repository. Routes by query intent (overview / implementation / relationship / comparison) and returns ranked code, README, folder, and module-doc chunks. Pass `intent` to override the classifier."
    )]
    async fn code_rag_search(
        &self,
        Parameters(params): Parameters<SearchParams>,
    ) -> Result<CallToolResult, McpError> {
        let SearchParams {
            query,
            intent: intent_override,
        } = params;
        let query = query.trim();
        if query.is_empty() {
            return Err(McpError::invalid_params("query must not be empty", None));
        }

        let (intent_used, hits) = self
            .run_search(query, intent_override.map(Into::into))
            .await?;

        let body = serde_json::to_string_pretty(&serde_json::json!({
            "intent": format!("{intent_used:?}"),
            "hits": hits,
        }))
        .map_err(|e| McpError::internal_error(format!("serialize failed: {e}"), None))?;

        Ok(CallToolResult::success(vec![Content::text(body)]))
    }

    #[tool(
        description = "Architecture/onboarding retrieval — forces Overview intent so README, crate, folder, and module-doc chunks surface ahead of function-level code. Use this when the question is 'how does this project work?' rather than 'how does X do Y?'."
    )]
    async fn code_rag_overview(
        &self,
        Parameters(params): Parameters<OverviewParams>,
    ) -> Result<CallToolResult, McpError> {
        let query = params
            .topic
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("project overview architecture");

        let (_intent, hits) = self.run_search(query, Some(QueryIntent::Overview)).await?;

        let body = serde_json::to_string_pretty(&serde_json::json!({
            "intent": "Overview (forced)",
            "hits": hits,
        }))
        .map_err(|e| McpError::internal_error(format!("serialize failed: {e}"), None))?;

        Ok(CallToolResult::success(vec![Content::text(body)]))
    }

    #[tool(
        description = "Call-graph traversal. Given a function identifier, returns its callers and/or callees as caller_chunk_id + caller_identifier + caller_file pairs. `direction` can be 'callers' (who calls X?), 'callees' (what does X call?), or 'both' (default)."
    )]
    async fn code_rag_graph(
        &self,
        Parameters(params): Parameters<GraphParams>,
    ) -> Result<CallToolResult, McpError> {
        let GraphParams {
            identifier,
            direction,
        } = params;
        let identifier = identifier.trim();
        if identifier.is_empty() {
            return Err(McpError::invalid_params(
                "identifier must not be empty",
                None,
            ));
        }

        let graph = self.load_call_graph().await?;
        let target = graph
            .unique_chunk_for_identifier(identifier)
            .map(str::to_string)
            .ok_or_else(|| {
                McpError::invalid_params(
                    format!(
                        "no unique chunk for identifier '{identifier}' — either not indexed or ambiguous"
                    ),
                    None,
                )
            })?;

        let dir = match direction {
            Some(GraphDirectionHint::Callers) => GraphDirection::Callers,
            Some(GraphDirectionHint::Callees) => GraphDirection::Callees,
            _ => GraphDirection::Both,
        };

        // Pull edges that touch the target so we can surface callee/caller
        // identifier + file metadata without a second lookup.
        let callers = self
            .state
            .store
            .get_callers(&target, None)
            .await
            .map_err(|e| McpError::internal_error(format!("get_callers failed: {e}"), None))?;
        let callees = self
            .state
            .store
            .get_callees(&target, None)
            .await
            .map_err(|e| McpError::internal_error(format!("get_callees failed: {e}"), None))?;

        let callers_view: Vec<GraphEdgeView> = callers
            .iter()
            .map(|e| GraphEdgeView {
                chunk_id: e.caller_chunk_id.clone(),
                identifier: e.caller_identifier.clone(),
                file: e.caller_file.clone(),
                resolution_tier: e.resolution_tier,
            })
            .collect();
        let callees_view: Vec<GraphEdgeView> = callees
            .iter()
            .map(|e| GraphEdgeView {
                chunk_id: e.callee_chunk_id.clone(),
                identifier: e.callee_identifier.clone(),
                file: e.callee_file.clone(),
                resolution_tier: e.resolution_tier,
            })
            .collect();

        let body = serde_json::to_string_pretty(&serde_json::json!({
            "target_identifier": identifier,
            "target_chunk_id": target,
            "direction": format!("{dir:?}"),
            "callers": if matches!(dir, GraphDirection::Callees) { Vec::<GraphEdgeView>::new() } else { callers_view },
            "callees": if matches!(dir, GraphDirection::Callers) { Vec::<GraphEdgeView>::new() } else { callees_view },
        }))
        .map_err(|e| McpError::internal_error(format!("serialize failed: {e}"), None))?;

        Ok(CallToolResult::success(vec![Content::text(body)]))
    }

    #[tool(
        description = "Expand the source window around a previously-returned chunk_id. Reads `window` lines centred on the chunk's start line. Saves Claude a full `Read` when a search hit's 30-line excerpt isn't enough context. Default window is 20."
    )]
    async fn code_rag_neighbors(
        &self,
        Parameters(params): Parameters<NeighborsParams>,
    ) -> Result<CallToolResult, McpError> {
        let NeighborsParams { chunk_id, window } = params;
        let window = window.unwrap_or(20).min(500); // cap to avoid dumping huge files
        if chunk_id.is_empty() {
            return Err(McpError::invalid_params("chunk_id must not be empty", None));
        }

        let chunks = self
            .state
            .store
            .get_chunks_by_ids(&[chunk_id.clone()])
            .await
            .map_err(|e| {
                McpError::internal_error(format!("get_chunks_by_ids failed: {e}"), None)
            })?;

        let chunk = chunks.into_iter().next().ok_or_else(|| {
            McpError::invalid_params(
                format!("chunk_id '{chunk_id}' not found in code_chunks (non-code chunk ids aren't supported yet)"),
                None,
            )
        })?;

        // Resolve the file on disk relative to repo_path. Index file_paths are
        // repo-relative forward-slash strings BUT when the ingest root was a
        // parent directory (portfolio layout), paths carry a project-name
        // prefix. Try the direct path first; on ENOENT, retry with the first
        // component stripped — handles both single-repo and portfolio DBs.
        let direct = std::path::Path::new(&self.repo_path).join(&chunk.file_path);
        let (full_path, content) = match tokio::fs::read_to_string(&direct).await {
            Ok(c) => (direct, c),
            Err(_) => {
                let stripped = chunk
                    .file_path
                    .split_once('/')
                    .map(|(_, rest)| rest.to_string())
                    .unwrap_or_else(|| chunk.file_path.clone());
                let fallback = std::path::Path::new(&self.repo_path).join(&stripped);
                let c = tokio::fs::read_to_string(&fallback).await.map_err(|e| {
                    McpError::internal_error(
                        format!(
                            "read failed for both {} and {}: {e}",
                            direct.display(),
                            fallback.display()
                        ),
                        None,
                    )
                })?;
                (fallback, c)
            }
        };
        let _ = &full_path; // keep path in scope for later logs if needed

        let lines: Vec<&str> = content.lines().collect();
        let total = lines.len();
        if total == 0 {
            return Ok(CallToolResult::success(vec![Content::text("(empty file)")]));
        }
        let anchor = chunk
            .start_line
            .saturating_sub(1)
            .min(total.saturating_sub(1));
        let half = window / 2;
        let start = anchor.saturating_sub(half);
        let end = (anchor + half).min(total.saturating_sub(1));

        let excerpt: String = (start..=end)
            .map(|i| format!("{:>5} | {}", i + 1, lines[i]))
            .collect::<Vec<_>>()
            .join("\n");

        let body = serde_json::to_string_pretty(&serde_json::json!({
            "chunk_id": chunk.chunk_id,
            "identifier": chunk.identifier,
            "file_path": chunk.file_path,
            "window_start_line": start + 1,
            "window_end_line": end + 1,
            "excerpt": excerpt,
        }))
        .map_err(|e| McpError::internal_error(format!("serialize failed: {e}"), None))?;

        Ok(CallToolResult::success(vec![Content::text(body)]))
    }

    #[tool(
        description = "Re-ingest the repo into the LanceDB index. Defaults to incremental mode — only files whose content_hash changed are re-embedded (typically single-digit seconds). Pass `mode: \"full\"` to wipe and rebuild the project's chunks from scratch (tens of seconds; use when the index looks corrupted). After a live edit, ask for `code_rag_reindex` to make the change visible to search; for a one-shot lookup of a just-edited symbol, prefer Grep."
    )]
    async fn code_rag_reindex(
        &self,
        Parameters(params): Parameters<ReindexParams>,
    ) -> Result<CallToolResult, McpError> {
        let started = std::time::Instant::now();
        let mode = params.mode.unwrap_or_default();
        let mode_str = match mode {
            ReindexMode::Incremental => "incremental",
            ReindexMode::Full => "full",
        };
        tracing::info!(
            repo = %self.repo_path,
            db = %self.db_path,
            mode = mode_str,
            "code_rag_reindex: spawning self with `ingest` subcommand"
        );

        let self_exe = std::env::current_exe().map_err(|e| {
            McpError::internal_error(format!("current_exe lookup failed: {e}"), None)
        })?;

        let mut args: Vec<&str> = vec!["ingest", &self.repo_path, "--db-path", &self.db_path];
        if !self.workspace_mode {
            args.push("--single-repo");
        }
        if mode == ReindexMode::Full {
            args.push("--full");
        }

        let output = tokio::process::Command::new(&self_exe)
            .args(&args)
            .output()
            .await
            .map_err(|e| {
                McpError::internal_error(
                    format!("failed to spawn `{}`: {e}", self_exe.display()),
                    None,
                )
            })?;

        let elapsed_ms = started.elapsed().as_millis();
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);

        if !output.status.success() {
            return Err(McpError::internal_error(
                format!(
                    "ingest exited with {:?} after {elapsed_ms}ms; stderr tail:\n{}",
                    output.status.code(),
                    tail(&stderr, 40)
                ),
                None,
            ));
        }

        // LanceDB tables are opened per-query in VectorStore — no explicit
        // reload needed to see the new data on the next tool call.
        let body = serde_json::to_string_pretty(&serde_json::json!({
            "status": "ok",
            "mode": mode_str,
            "elapsed_ms": elapsed_ms,
            "repo_path": self.repo_path,
            "db_path": self.db_path,
            "stdout_tail": tail(&stdout, 20),
            "stderr_tail": tail(&stderr, 20),
        }))
        .map_err(|e| McpError::internal_error(format!("serialize failed: {e}"), None))?;

        Ok(CallToolResult::success(vec![Content::text(body)]))
    }
}

/// Reindex mode for `code_rag_reindex`. Default is `incremental`.
#[derive(Debug, Deserialize, JsonSchema, Default, PartialEq)]
#[serde(rename_all = "lowercase")]
enum ReindexMode {
    /// Diff against the existing index — only re-embed files whose
    /// content_hash changed. Fast (single-digit seconds for a one-file
    /// edit). The default; use this after live edits in a session.
    #[default]
    Incremental,
    /// Wipe the project's chunks and re-embed everything from scratch.
    /// Slower (tens of seconds) but the recovery path when the index
    /// looks corrupted or the chunk-derivation pipeline has changed.
    Full,
}

/// Parameters for `code_rag_reindex`.
#[derive(Debug, Deserialize, JsonSchema, Default)]
struct ReindexParams {
    /// Reindex mode. `incremental` (default) only re-embeds changed
    /// files; `full` wipes and rebuilds the project.
    #[serde(default)]
    mode: Option<ReindexMode>,
}

fn tail(s: &str, n: usize) -> String {
    let lines: Vec<&str> = s.lines().collect();
    let start = lines.len().saturating_sub(n);
    lines[start..].join("\n")
}

#[tool_handler]
impl ServerHandler for CodeRagServer {
    fn get_info(&self) -> ServerInfo {
        // `Implementation::from_build_env()` reads rmcp's own CARGO_PKG vars
        // (since it's defined inside the rmcp crate), so we construct ours
        // explicitly from this binary's compile-time env.
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(
                env!("CARGO_PKG_NAME"),
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(
                "Intent-routed semantic retrieval over an indexed repo. \
                 Prefer Grep/Read for exact identifiers and just-edited code; \
                 reach for code_rag_search on conceptual queries.",
            )
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // stdout is the MCP protocol channel — route all log output to stderr.
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,lance::file_audit=warn")),
        )
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    // Bare run (`code-rag-mcp` with no args at all) is the install path —
    // double-click in Explorer, or run from a terminal with no flags. We
    // detect it pre-clap so it stays unambiguous.
    if std::env::args().len() == 1 {
        // Run setup, then pause on a TTY so double-click users on Windows
        // can read the output before the console window closes. Claude
        // Code's spawn case never reaches here (it always passes args), so
        // the pause is safe to gate purely on TTY-ness.
        let result = run_yaml_setup();
        if let Err(ref e) = result {
            eprintln!("\nError: {e}");
        }
        if std::io::IsTerminal::is_terminal(&std::io::stdin()) {
            println!("\nPress Enter to close...");
            let mut buf = String::new();
            let _ = std::io::stdin().read_line(&mut buf);
        }
        return result;
    }

    let cli = Cli::parse();

    // Subcommand dispatch — `ingest` runs the same pipeline as the standalone
    // `code-raptor` CLI used to (just shipped under the same exe), then exits.
    if let Some(Commands::Ingest {
        repo_path,
        db_path,
        project_name,
        single_repo,
        full,
    }) = cli.command
    {
        return code_raptor::ingest_repo(code_raptor::IngestOpts {
            repo_path,
            db_path,
            project_name,
            single_repo,
            full,
            dry_run: false,
        })
        .await;
    }

    tracing::info!(db_path = %cli.db_path, "starting code-rag MCP server");

    // AppState::from_config wires up an LlmClient whose constructor panics if
    // GEMINI_API_KEY is unset. The MCP surface never calls the LLM (Claude
    // Code is the LLM — we're only the retriever), so inject a placeholder if
    // the user hasn't set one. External users shouldn't need an API key to
    // run retrieval. SAFETY: set_var is safe here because it runs before any
    // other thread is spawned by tokio::main's runtime.
    if std::env::var_os("GEMINI_API_KEY").is_none() {
        // SAFETY: single-threaded at this point; no other reader of the env.
        unsafe {
            std::env::set_var("GEMINI_API_KEY", "unused-mcp-does-not-call-llm");
        }
    }

    let state = AppState::from_config(&cli.db_path, &cli.model, !cli.no_rerank)
        .await
        .map_err(|e| anyhow::anyhow!("AppState init failed: {e}"))?;

    let server = CodeRagServer::new(state, cli.repo_path, cli.db_path, cli.workspace);
    let service = server.serve(stdio()).await.inspect_err(|e| {
        tracing::error!("serve error: {e:?}");
    })?;

    service.waiting().await?;
    Ok(())
}

/// YAML config consumed by the bare-run setup mode. Lives next to the exe.
#[derive(Debug, Deserialize, Serialize)]
struct SetupConfig {
    /// Path to the repo (or umbrella folder) to set up. Use "." if the exe is
    /// being run from inside the target dir.
    target_path: String,

    /// True if `target_path` is a parent folder containing many sibling
    /// sub-projects (each becomes its own project in the index).
    #[serde(default)]
    workspace: bool,
}

const CONFIG_FILE: &str = "code-rag-mcp.config.yaml";

const DEFAULT_CONFIG: &str = "\
# code-rag-mcp setup config — edit this file, then run code-rag-mcp again.
#
# When you run code-rag-mcp.exe with no arguments (e.g. by double-clicking),
# it reads this file from the same directory as the exe and sets up the
# directory at `target_path` for use with Claude Code:
#   - writes `.claude/skills/code-rag.md`
#   - registers `code-rag` in `.mcp.json` (merging with any existing servers)
#   - appends `.code-rag-mcp/` to `.gitignore`
# The first ingest happens later, automatically — when you open Claude Code
# in the target directory and ask any conceptual question.

# REQUIRED. Path to the repo (or umbrella folder) you want indexed.
# Examples:
#   target_path: .                              # the dir the exe is in
#   target_path: C:/Users/me/projects/my-repo
#   target_path: /home/me/projects/my-repo
target_path: .

# OPTIONAL. Set to true if `target_path` is a parent folder containing many
# sibling sub-projects (each becomes its own project in the index). Default
# false (single repo).
workspace: false
";

/// Bare-run install path: `code-rag-mcp` with no args.
///
/// Looks for `code-rag-mcp.config.yaml` next to the exe. If missing, writes
/// the default template there and exits with instructions. If present, parses
/// it and runs setup against `target_path`: writes the Claude Code skill,
/// registers `code-rag` in `.mcp.json` (merging with any existing servers),
/// and appends `.code-rag-mcp/` to `.gitignore`.
fn run_yaml_setup() -> Result<()> {
    let exe = std::env::current_exe()?;
    let exe_dir = exe
        .parent()
        .ok_or_else(|| anyhow::anyhow!("exe has no parent dir"))?;
    let config_path = exe_dir.join(CONFIG_FILE);

    if !config_path.exists() {
        std::fs::write(&config_path, DEFAULT_CONFIG)?;
        println!("First run — wrote default config to:");
        println!("  {}", config_path.display());
        println!();
        println!("Edit `target_path` (and `workspace` if needed), then run this exe again.");
        return Ok(());
    }

    let raw = std::fs::read_to_string(&config_path)?;
    let cfg: SetupConfig = serde_yaml::from_str(&raw)
        .map_err(|e| anyhow::anyhow!("parse {} failed: {e}", config_path.display()))?;

    let target = std::path::Path::new(&cfg.target_path);
    if !target.is_dir() {
        anyhow::bail!(
            "target_path in {} does not exist: {}",
            config_path.display(),
            target.display()
        );
    }

    println!("code-rag-mcp setup → {}", target.display());
    if cfg.workspace {
        println!("  (workspace mode: each top-level subdirectory becomes its own project)");
    }

    // 1. Skill file. Bundled into the binary at compile time so we don't need
    //    a source-tree dependency at runtime.
    let skill_dir = target.join(".claude").join("skills");
    std::fs::create_dir_all(&skill_dir)?;
    let skill_path = skill_dir.join("code-rag.md");
    let skill_existed = skill_path.exists();
    std::fs::write(&skill_path, BUNDLED_SKILL)?;
    println!(
        "  {} {}",
        if skill_existed { "updated" } else { "wrote  " },
        skill_path.display()
    );

    // 2. .mcp.json — merge with any existing `mcpServers`.
    let mcp_path = target.join(".mcp.json");
    let mut mcp: serde_json::Value = if mcp_path.exists() {
        serde_json::from_str(&std::fs::read_to_string(&mcp_path)?)
            .map_err(|e| anyhow::anyhow!("parse {} failed: {e}", mcp_path.display()))?
    } else {
        serde_json::json!({ "mcpServers": {} })
    };
    let servers = mcp
        .as_object_mut()
        .and_then(|m| {
            m.entry("mcpServers")
                .or_insert_with(|| serde_json::json!({}))
                .as_object_mut()
        })
        .ok_or_else(|| anyhow::anyhow!("malformed .mcp.json: mcpServers is not an object"))?;
    let mut args: Vec<serde_json::Value> = vec![
        "--db-path".into(),
        "./.code-rag-mcp/index.lance".into(),
        "--repo-path".into(),
        ".".into(),
    ];
    if cfg.workspace {
        args.push("--workspace".into());
    }
    // Use the absolute path of the running exe so Claude Code doesn't depend
    // on PATH being set up. JSON tolerates forward slashes on Windows, so we
    // normalize to avoid backslash-escape pain in the generated file.
    let command_path = exe.to_string_lossy().replace('\\', "/");
    let server_existed = servers.contains_key("code-rag");
    servers.insert(
        "code-rag".into(),
        serde_json::json!({
            "command": command_path,
            "args": args,
        }),
    );
    std::fs::write(&mcp_path, serde_json::to_string_pretty(&mcp)? + "\n")?;
    println!(
        "  {} {} (code-rag entry)",
        if server_existed { "updated" } else { "wrote  " },
        mcp_path.display()
    );

    // 3. .gitignore — append index dir if missing.
    let ignore_entry = ".code-rag-mcp/";
    let gitignore_path = target.join(".gitignore");
    let existing = std::fs::read_to_string(&gitignore_path).unwrap_or_default();
    let already_present = existing
        .lines()
        .any(|l| l.trim() == ignore_entry || l.trim() == ignore_entry.trim_end_matches('/'));
    if already_present {
        println!(
            "  ok      {} ({} already listed)",
            gitignore_path.display(),
            ignore_entry
        );
    } else {
        let mut new = existing.clone();
        if !new.is_empty() && !new.ends_with('\n') {
            new.push('\n');
        }
        new.push_str(&format!(
            "\n# code-rag MCP index — large binary, never commit\n{ignore_entry}\n"
        ));
        std::fs::write(&gitignore_path, new)?;
        println!("  added   {ignore_entry} → {}", gitignore_path.display());
    }

    println!();
    println!("Setup complete. Next:");
    println!("  1. Open Claude Code in {}.", target.display());
    println!("  2. Ask any conceptual question — the skill instructs the agent to run");
    println!("     `code_rag_reindex mode=full` for the initial ingest automatically.");
    println!();
    println!("(Re-run this exe any time to re-apply the config.)");

    Ok(())
}
