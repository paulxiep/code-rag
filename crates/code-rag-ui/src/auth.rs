//! Authentication for Gemini API — OAuth2 PKCE (primary) + API key (fallback).

use gloo_storage::{LocalStorage, Storage};
use serde::{Deserialize, Serialize};

const STORAGE_KEY: &str = "code-rag-auth";

/// How to authenticate with the Gemini API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AuthMethod {
    ApiKey(String),
    OAuth2 {
        access_token: String,
        /// Unix timestamp in seconds when the token expires.
        expires_at: Option<u64>,
    },
}

impl AuthMethod {
    /// Check if the current auth method is expired or empty.
    pub fn is_valid(&self) -> bool {
        match self {
            AuthMethod::ApiKey(key) => !key.is_empty(),
            AuthMethod::OAuth2 { access_token, expires_at } => {
                if access_token.is_empty() {
                    return false;
                }
                if let Some(exp) = expires_at {
                    let now = (js_sys::Date::now() / 1000.0) as u64;
                    now < *exp
                } else {
                    true
                }
            }
        }
    }
}

/// Load saved auth from localStorage.
pub fn load_auth() -> Option<AuthMethod> {
    LocalStorage::get::<AuthMethod>(STORAGE_KEY).ok().filter(|a| a.is_valid())
}

/// Save auth to localStorage.
pub fn save_auth(auth: &AuthMethod) {
    let _ = LocalStorage::set(STORAGE_KEY, auth);
}

/// Clear saved auth.
pub fn clear_auth() {
    LocalStorage::delete(STORAGE_KEY);
}

// --- OAuth2 PKCE helpers ---

/// Generate a random code verifier for PKCE (43-128 chars, URL-safe).
pub fn generate_code_verifier() -> String {
    let window = web_sys::window().expect("no window");
    let crypto = window.crypto().expect("no crypto");

    let mut buf = [0u8; 32];
    crypto
        .get_random_values_with_u8_array(&mut buf)
        .expect("get_random_values failed");

    base64_url_encode(&buf)
}

/// Compute SHA-256 code challenge from verifier (for PKCE S256 method).
pub async fn compute_code_challenge(verifier: &str) -> Result<String, String> {
    let window = web_sys::window().ok_or("no window")?;
    let crypto = window.crypto().map_err(|_| "no crypto")?;
    let subtle = crypto.subtle();

    let data = js_sys::Uint8Array::from(verifier.as_bytes());

    let hash_promise = subtle
        .digest_with_str_and_buffer_source("SHA-256", &data)
        .map_err(|_| "digest failed")?;

    let hash_value = wasm_bindgen_futures::JsFuture::from(hash_promise)
        .await
        .map_err(|_| "hash await failed")?;

    let hash_buffer = js_sys::Uint8Array::new(&hash_value);
    let mut hash_bytes = vec![0u8; hash_buffer.length() as usize];
    hash_buffer.copy_to(&mut hash_bytes);

    Ok(base64_url_encode(&hash_bytes))
}

/// Build the Google OAuth2 authorization URL for PKCE flow.
pub fn build_auth_url(client_id: &str, redirect_uri: &str, code_challenge: &str) -> String {
    format!(
        "https://accounts.google.com/o/oauth2/v2/auth\
        ?client_id={client_id}\
        &redirect_uri={redirect_uri}\
        &response_type=code\
        &scope=https://www.googleapis.com/auth/generative-language\
        &code_challenge={code_challenge}\
        &code_challenge_method=S256\
        &access_type=online\
        &prompt=consent"
    )
}

/// Exchange authorization code for access token (PKCE, no client secret).
pub async fn exchange_code_for_token(
    code: &str,
    client_id: &str,
    redirect_uri: &str,
    code_verifier: &str,
) -> Result<AuthMethod, String> {
    use gloo_net::http::Request;

    let body = format!(
        "code={code}\
        &client_id={client_id}\
        &redirect_uri={redirect_uri}\
        &code_verifier={code_verifier}\
        &grant_type=authorization_code"
    );

    let resp = Request::post("https://oauth2.googleapis.com/token")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body)
        .map_err(|e| format!("Failed to build token request: {e}"))?
        .send()
        .await
        .map_err(|e| format!("Token exchange failed: {e}"))?;

    if !resp.ok() {
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("Token exchange error ({}): {text}", resp.status()));
    }

    #[derive(Deserialize)]
    struct TokenResponse {
        access_token: String,
        expires_in: Option<u64>,
    }

    let token: TokenResponse = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse token response: {e}"))?;

    let expires_at = token
        .expires_in
        .map(|secs| (js_sys::Date::now() / 1000.0) as u64 + secs);

    Ok(AuthMethod::OAuth2 {
        access_token: token.access_token,
        expires_at,
    })
}

/// Base64url-encode bytes (no padding, URL-safe).
fn base64_url_encode(bytes: &[u8]) -> String {
    let mut encoded = String::new();
    // Simple base64url without external dep
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

    let mut i = 0;
    while i < bytes.len() {
        let b0 = bytes[i] as u32;
        let b1 = if i + 1 < bytes.len() { bytes[i + 1] as u32 } else { 0 };
        let b2 = if i + 2 < bytes.len() { bytes[i + 2] as u32 } else { 0 };

        let triple = (b0 << 16) | (b1 << 8) | b2;

        encoded.push(CHARS[((triple >> 18) & 0x3F) as usize] as char);
        encoded.push(CHARS[((triple >> 12) & 0x3F) as usize] as char);

        if i + 1 < bytes.len() {
            encoded.push(CHARS[((triple >> 6) & 0x3F) as usize] as char);
        }
        if i + 2 < bytes.len() {
            encoded.push(CHARS[(triple & 0x3F) as usize] as char);
        }

        i += 3;
    }

    encoded
}
