//! Auth panel — Google OAuth2 sign-in (GIS popup) + API key fallback.

use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use crate::auth::{self, AuthMethod};

const GOOGLE_CLIENT_ID: Option<&str> = option_env!("GOOGLE_OAUTH_CLIENT_ID");

#[component]
pub fn AuthPanel() -> impl IntoView {
    let auth_signal =
        use_context::<RwSignal<Option<AuthMethod>>>().expect("AuthSignal not provided");

    let (show_key_input, set_show_key_input) = signal(false);
    let (api_key_input, set_api_key_input) = signal(String::new());
    let (error, set_error) = signal(Option::<String>::None);

    let on_google_sign_in = move |_| {
        let client_id = match GOOGLE_CLIENT_ID {
            Some(id) => id,
            None => {
                set_error.set(Some("OAuth client ID not configured".into()));
                return;
            }
        };

        spawn_local(async move {
            match auth::request_google_token(client_id).await {
                Ok(auth_method) => {
                    auth::save_auth(&auth_method);
                    auth_signal.set(Some(auth_method));
                    set_error.set(None);
                }
                Err(e) => {
                    set_error.set(Some(e));
                }
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
        if let Some(AuthMethod::OAuth2 { ref access_token, .. }) = auth_signal.get() {
            auth::revoke_token(access_token);
        }
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
