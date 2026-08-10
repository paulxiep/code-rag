//! R5: fetch + light-parse the per-project topology viz artifact.
//!
//! The artifact (`static/viz/graph_viz_<project>.json`, schema v1) is written
//! natively by `code-raptor::viz` — that module is the schema's source of
//! truth. Only `meta`-level fields and `communities` are deserialized here
//! (for the legend); the full node/edge payload is handed to `graph.js` as a
//! raw string, avoiding a pointless serde round-trip of ~5000 nodes.

use gloo_net::http::Request;
use serde::Deserialize;

/// The schema this UI understands; mismatched artifacts are rejected.
const SCHEMA_VERSION: u32 = 1;

/// Legend-level view of the artifact (unknown fields — nodes, edges — are
/// ignored by serde, which is what makes this parse "light").
#[derive(Debug, Clone, Deserialize)]
pub struct VizMeta {
    pub schema_version: u32,
    #[allow(dead_code)]
    pub project: String,
    pub node_total: usize,
    pub edge_total: usize,
    #[serde(default)]
    pub truncated: bool,
    #[serde(default)]
    pub communities: Vec<VizCommunity>,
}

/// One legend row (mirrors `code-raptor::viz::VizCommunity`).
#[derive(Debug, Clone, Deserialize)]
pub struct VizCommunity {
    pub id: u32,
    pub size: usize,
    #[serde(default)]
    pub cohesion: f32,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub dir: String,
}

/// The node payload `graph.js` passes back on click (mirrors
/// `code-raptor::viz::VizNode`). Consumed by the U4 click-to-query flow.
#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub struct VizNodeClick {
    #[allow(dead_code)]
    pub id: String,
    pub label: String,
    pub file: String,
    pub kind: String,
    #[serde(default)]
    #[allow(dead_code)]
    pub community: Option<u32>,
    #[serde(default)]
    #[allow(dead_code)]
    pub degree: f64,
}

/// Why an artifact fetch produced nothing renderable.
#[derive(Debug, Clone, PartialEq)]
pub enum VizFetchError {
    /// 404 — the project has no topology artifact (not an error state).
    Missing,
    Other(String),
}

/// Project name → filename component. MUST mirror `code-raptor`'s
/// `sanitize_project` (lib.rs) — the native side writes with that rule, this
/// side fetches with it.
fn sanitize_project(project: &str) -> String {
    project
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Fetch one project's viz artifact: raw JSON (for `graph.js`) + parsed meta
/// (for the legend). Relative URL so `--public-url /code-rag/` keeps working.
pub async fn fetch_viz(project: &str) -> Result<(String, VizMeta), VizFetchError> {
    let url = format!("static/viz/graph_viz_{}.json", sanitize_project(project));
    let resp = Request::get(&url)
        .send()
        .await
        .map_err(|e| VizFetchError::Other(format!("Fetch failed: {e}")))?;
    if resp.status() == 404 {
        return Err(VizFetchError::Missing);
    }
    if !resp.ok() {
        return Err(VizFetchError::Other(format!(
            "Fetch failed ({})",
            resp.status()
        )));
    }
    let text = resp
        .text()
        .await
        .map_err(|e| VizFetchError::Other(format!("Read failed: {e}")))?;
    let meta: VizMeta = serde_json::from_str(&text)
        .map_err(|e| VizFetchError::Other(format!("Parse failed: {e}")))?;
    if meta.schema_version != SCHEMA_VERSION {
        return Err(VizFetchError::Other(format!(
            "Unsupported viz schema v{} (expected v{SCHEMA_VERSION})",
            meta.schema_version
        )));
    }
    Ok((text, meta))
}
