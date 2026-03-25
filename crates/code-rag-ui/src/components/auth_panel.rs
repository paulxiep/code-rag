//! Auth panel — Google OAuth2 sign-in + API key fallback.

use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use crate::auth::{self, AuthMethod};

const GOOGLE_CLIENT_ID: Option<&str> = option_env!("GOOGLE_OAUTH_CLIENT_ID");
const PKCE_VERIFIER_KEY: &str = "code-rag-pkce-verifier";

#[component]
pub fn AuthPanel() -> impl IntoView {
    let auth_signal =
        use_context::<RwSignal<Option<AuthMethod>>>().expect("AuthSignal not provided");

    let (show_key_input, set_show_key_input) = signal(false);
    let (api_key_input, set_api_key_input) = signal(String::new());
    let (error, set_error) = signal(Option::<String>::None);

    // Check for OAuth callback on mount
    spawn_local({
        let auth_signal = auth_signal;
        async move {
            if let Some(auth) = handle_oauth_callback().await {
                auth::save_auth(&auth);
                auth_signal.set(Some(auth));
                // Clean URL
                if let Some(window) = web_sys::window() {
                    let _ = window
                        .history()
                        .ok()
                        .and_then(|h| {
                            let loc = window.location();
                            let clean = format!(
                                "{}{}",
                                loc.origin().unwrap_or_default(),
                                loc.pathname().unwrap_or_default()
                            );
                            h.replace_state_with_url(&wasm_bindgen::JsValue::NULL, "", Some(&clean))
                                .ok()
                        });
                }
            }
        }
    });

    let on_google_sign_in = move |_| {
        let client_id = match GOOGLE_CLIENT_ID {
            Some(id) => id,
            None => {
                set_error.set(Some("OAuth client ID not configured".into()));
                return;
            }
        };

        spawn_local(async move {
            let verifier = auth::generate_code_verifier();
            let challenge = match auth::compute_code_challenge(&verifier).await {
                Ok(c) => c,
                Err(e) => {
                    set_error.set(Some(e));
                    return;
                }
            };

            // Store verifier for callback
            if let Ok(Some(storage)) = web_sys::window()
                .ok_or("no window")
                .and_then(|w| w.local_storage().map_err(|_| "no storage"))
            {
                let _ = storage.set_item(PKCE_VERIFIER_KEY, &verifier);
            }

            let redirect_uri = get_redirect_uri();
            let url = auth::build_auth_url(client_id, &redirect_uri, &challenge);

            if let Some(window) = web_sys::window() {
                let _ = window.location().set_href(&url);
            }
        });
    };

    let on_api_key_submit = move |_| {
        let key = api_key_input.get().trim().to_string();
        if key.is_empty() {
            return;
        }
        let auth = AuthMethod::ApiKey(key);
        auth::save_auth(&auth);
        auth_signal.set(Some(auth));
        set_show_key_input.set(false);
    };

    let on_sign_out = move |_| {
        auth::clear_auth();
        auth_signal.set(None);
    };

    view! {
        <div class="auth-panel">
            {move || {
                let current_auth = auth_signal.get();
                match current_auth {
                    Some(AuthMethod::OAuth2 { .. }) => {
                        view! {
                            <span class="auth-status">"Signed in (Google)"</span>
                            <button class="auth-btn sign-out" on:click=on_sign_out>"Sign out"</button>
                        }
                            .into_any()
                    }
                    Some(AuthMethod::ApiKey(_)) => {
                        view! {
                            <span class="auth-status">"Using API key"</span>
                            <button class="auth-btn sign-out" on:click=on_sign_out>"Remove"</button>
                        }
                            .into_any()
                    }
                    None => {
                        view! {
                            <Show when=move || GOOGLE_CLIENT_ID.is_some()>
                                <button class="auth-btn google" on:click=on_google_sign_in>
                                    "Sign in with Google"
                                </button>
                            </Show>
                            <button
                                class="auth-btn api-key"
                                on:click=move |_| set_show_key_input.update(|v| *v = !*v)
                            >
                                "Use API key"
                            </button>
                            <Show when=move || show_key_input.get()>
                                <div class="api-key-input">
                                    <input
                                        type="password"
                                        placeholder="Gemini API key"
                                        prop:value=move || api_key_input.get()
                                        on:input=move |ev| {
                                            let target = event_target::<web_sys::HtmlInputElement>(&ev);
                                            set_api_key_input.set(target.value());
                                        }
                                        on:keydown=move |ev: web_sys::KeyboardEvent| {
                                            if ev.key() == "Enter" {
                                                on_api_key_submit(());
                                            }
                                        }
                                    />
                                    <button class="auth-btn" on:click=move |_| on_api_key_submit(())>"Save"</button>
                                </div>
                            </Show>
                            <Show when=move || error.get().is_some()>
                                <span class="auth-error">{move || error.get().unwrap_or_default()}</span>
                            </Show>
                        }
                            .into_any()
                    }
                }
            }}
        </div>
    }
}

fn get_redirect_uri() -> String {
    web_sys::window()
        .and_then(|w| {
            let loc = w.location();
            let origin = loc.origin().ok()?;
            let pathname = loc.pathname().ok()?;
            Some(format!("{}{}", origin, pathname))
        })
        .unwrap_or_default()
}

/// Check URL for OAuth2 callback code and exchange it for a token.
async fn handle_oauth_callback() -> Option<AuthMethod> {
    let window = web_sys::window()?;
    let search = window.location().search().ok()?;

    if !search.contains("code=") {
        return None;
    }

    let params = web_sys::UrlSearchParams::new_with_str(&search).ok()?;
    let code = params.get("code")?;

    let client_id = GOOGLE_CLIENT_ID?;

    // Retrieve stored PKCE verifier
    let storage = window.local_storage().ok()??;
    let verifier = storage.get_item(PKCE_VERIFIER_KEY).ok()??;
    storage.remove_item(PKCE_VERIFIER_KEY).ok();

    let redirect_uri = get_redirect_uri();

    match auth::exchange_code_for_token(&code, client_id, &redirect_uri, &verifier).await {
        Ok(auth) => Some(auth),
        Err(_) => None,
    }
}
