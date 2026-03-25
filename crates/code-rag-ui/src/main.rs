mod api;
mod components;

#[cfg(feature = "standalone")]
mod auth;
#[cfg(feature = "standalone")]
mod data;
#[cfg(feature = "standalone")]
mod gemini;
#[cfg(feature = "standalone")]
mod search;
#[cfg(feature = "standalone")]
mod standalone_api;

use leptos::prelude::*;
use leptos::mount::mount_to_body;
use wasm_bindgen_futures::spawn_local;

use components::ChatView;

fn main() {
    console_error_panic_hook::set_once();
    mount_to_body(App);
}

#[component]
fn App() -> impl IntoView {
    let api_base = api_base_url();
    let (projects, set_projects) = signal(Vec::<String>::new());

    // Fetch projects on mount
    let base = api_base.clone();
    spawn_local(async move {
        if let Ok(p) = api::fetch_projects(&base).await {
            set_projects.set(p);
        }
    });

    view! {
        <div class="app">
            <header class="app-header">
                <h1>"Code RAG Chat"</h1>
                <p>"Ask questions about the indexed codebase"</p>
            </header>

            <Show when=move || !projects.get().is_empty()>
                <div class="projects-bar">
                    <For
                        each=move || projects.get()
                        key=|p| p.clone()
                        let:project
                    >
                        <span class="project-tag">{project}</span>
                    </For>
                </div>
            </Show>

            <ChatView api_base=api_base />
        </div>
    }
}

/// Determine the API base URL.
/// In dev (trunk serve), the proxy forwards to the Axum backend.
/// In production, the API is on the same origin.
fn api_base_url() -> String {
    web_sys::window()
        .and_then(|w| w.location().origin().ok())
        .unwrap_or_else(|| String::from("http://127.0.0.1:3000"))
}
