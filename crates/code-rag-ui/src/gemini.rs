//! Direct Gemini REST API client for in-browser use.

use gloo_net::http::Request;
use serde::{Deserialize, Serialize};

use crate::auth::AuthMethod;

const GEMINI_MODEL: &str = "gemini-2.5-flash";
const API_BASE: &str = "https://generativelanguage.googleapis.com/v1beta";

#[derive(Serialize)]
struct GenerateRequest {
    contents: Vec<Content>,
}

#[derive(Serialize)]
struct Content {
    parts: Vec<Part>,
}

#[derive(Serialize)]
struct Part {
    text: String,
}

#[derive(Deserialize)]
struct GenerateResponse {
    candidates: Option<Vec<Candidate>>,
}

#[derive(Deserialize)]
struct Candidate {
    content: Option<CandidateContent>,
}

#[derive(Deserialize)]
struct CandidateContent {
    parts: Option<Vec<CandidatePart>>,
}

#[derive(Deserialize)]
struct CandidatePart {
    text: Option<String>,
}

/// Call the Gemini API to generate a response.
pub async fn generate(prompt: &str, auth: &AuthMethod) -> Result<String, String> {
    let body = GenerateRequest {
        contents: vec![Content {
            parts: vec![Part {
                text: prompt.to_string(),
            }],
        }],
    };

    let json_body =
        serde_json::to_string(&body).map_err(|e| format!("Failed to serialize request: {e}"))?;

    let request = match auth {
        AuthMethod::ApiKey(key) => {
            let url = format!("{API_BASE}/models/{GEMINI_MODEL}:generateContent?key={key}");
            Request::post(&url)
                .header("Content-Type", "application/json")
                .body(json_body)
                .map_err(|e| format!("Failed to build request: {e}"))?
        }
        AuthMethod::OAuth2 { access_token, .. } => {
            let url = format!("{API_BASE}/models/{GEMINI_MODEL}:generateContent");
            Request::post(&url)
                .header("Content-Type", "application/json")
                .header("Authorization", &format!("Bearer {access_token}"))
                .body(json_body)
                .map_err(|e| format!("Failed to build request: {e}"))?
        }
    };

    let resp = request
        .send()
        .await
        .map_err(|e| format!("Gemini API request failed: {e}"))?;

    if !resp.ok() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("Gemini API error ({status}): {text}"));
    }

    let gen_resp: GenerateResponse = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse Gemini response: {e}"))?;

    gen_resp
        .candidates
        .and_then(|c| c.into_iter().next())
        .and_then(|c| c.content)
        .and_then(|c| c.parts)
        .and_then(|p| p.into_iter().next())
        .and_then(|p| p.text)
        .ok_or_else(|| "Empty response from Gemini".to_string())
}
