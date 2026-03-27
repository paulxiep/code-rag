//! Authentication for Gemini API — OAuth2 via GIS popup (primary) + API key (fallback).

use gloo_storage::{LocalStorage, Storage};
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

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

// --- Google Identity Services (popup flow) ---

#[wasm_bindgen(inline_js = "
export function gis_request_token(client_id, scope) {
    return new Promise((resolve, reject) => {
        if (typeof google === 'undefined' || !google.accounts || !google.accounts.oauth2) {
            reject('Google Identity Services library not loaded');
            return;
        }
        const client = google.accounts.oauth2.initTokenClient({
            client_id,
            scope,
            callback: (resp) => {
                if (resp.error) {
                    reject(resp.error_description || resp.error);
                } else {
                    resolve(JSON.stringify({
                        access_token: resp.access_token,
                        expires_in: resp.expires_in || 3600,
                    }));
                }
            },
            error_callback: (err) => {
                reject(err.message || 'Popup closed or blocked');
            },
        });
        client.requestAccessToken();
    });
}

export function gis_revoke(access_token) {
    if (typeof google !== 'undefined' && google.accounts && google.accounts.oauth2) {
        google.accounts.oauth2.revoke(access_token);
    }
}
")]
extern "C" {
    #[wasm_bindgen(catch)]
    async fn gis_request_token(client_id: &str, scope: &str) -> Result<JsValue, JsValue>;

    fn gis_revoke(access_token: &str);
}

const SCOPE: &str = "https://www.googleapis.com/auth/generative-language.peruserquota https://www.googleapis.com/auth/generative-language.retriever";

/// Open the Google sign-in popup and return an OAuth2 credential.
pub async fn request_google_token(client_id: &str) -> Result<AuthMethod, String> {
    let result = gis_request_token(client_id, SCOPE)
        .await
        .map_err(|e| e.as_string().unwrap_or_else(|| "OAuth popup failed".into()))?;

    let json = result.as_string().ok_or("unexpected response from GIS")?;

    #[derive(Deserialize)]
    struct TokenResponse {
        access_token: String,
        expires_in: u64,
    }

    let token: TokenResponse =
        serde_json::from_str(&json).map_err(|e| format!("Failed to parse token: {e}"))?;

    let expires_at = (js_sys::Date::now() / 1000.0) as u64 + token.expires_in;

    Ok(AuthMethod::OAuth2 {
        access_token: token.access_token,
        expires_at: Some(expires_at),
    })
}

/// Revoke an OAuth token.
pub fn revoke_token(access_token: &str) {
    gis_revoke(access_token);
}
