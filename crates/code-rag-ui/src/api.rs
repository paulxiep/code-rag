#[cfg(not(feature = "standalone"))]
use gloo_net::http::Request;
use serde::Deserialize;
#[cfg(not(feature = "standalone"))]
use serde::Serialize;

/// POST /chat request
#[cfg(not(feature = "standalone"))]
#[derive(Debug, Serialize)]
pub struct ChatRequest {
    pub query: String,
}

/// POST /chat response
#[derive(Debug, Clone, Deserialize)]
pub struct ChatResponse {
    pub answer: String,
    pub sources: Vec<SourceInfo>,
    pub intent: String,
}

/// Source reference in response
#[derive(Debug, Clone, Deserialize)]
pub struct SourceInfo {
    #[serde(rename = "type")]
    pub chunk_type: String,
    #[serde(default)]
    #[allow(dead_code)]
    pub chunk_id: String,
    pub path: String,
    pub label: String,
    pub project: String,
    #[allow(dead_code)]
    pub relevance: f32,
    pub relevance_pct: u8,
    pub line: usize,
}

/// GET /projects response
#[cfg(not(feature = "standalone"))]
#[derive(Debug, Clone, Deserialize)]
pub struct ProjectsResponse {
    pub projects: Vec<String>,
}

/// Send a chat query to the backend API.
#[cfg(not(feature = "standalone"))]
pub async fn send_chat(base_url: &str, query: &str) -> Result<ChatResponse, String> {
    let url = format!("{}/chat", base_url);
    let body = ChatRequest {
        query: query.to_string(),
    };

    let resp = Request::post(&url)
        .header("Content-Type", "application/json")
        .json(&body)
        .map_err(|e| format!("Failed to build request: {e}"))?
        .send()
        .await
        .map_err(|e| format!("Request failed: {e}"))?;

    if !resp.ok() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("API error ({status}): {text}"));
    }

    resp.json::<ChatResponse>()
        .await
        .map_err(|e| format!("Failed to parse response: {e}"))
}

/// Fetch the list of indexed projects.
#[cfg(not(feature = "standalone"))]
pub async fn fetch_projects(base_url: &str) -> Result<Vec<String>, String> {
    let url = format!("{}/projects", base_url);

    let resp = Request::get(&url)
        .send()
        .await
        .map_err(|e| format!("Request failed: {e}"))?;

    if !resp.ok() {
        return Err(format!("API error ({})", resp.status()));
    }

    let body = resp
        .json::<ProjectsResponse>()
        .await
        .map_err(|e| format!("Failed to parse response: {e}"))?;

    Ok(body.projects)
}
