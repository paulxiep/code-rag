mod api;
mod components;

#[cfg(feature = "standalone")]
mod auth;
#[cfg(feature = "standalone")]
mod data;
#[cfg(feature = "standalone")]
mod embedder;
#[cfg(feature = "standalone")]
mod gemini;
#[cfg(feature = "standalone")]
mod search;
#[cfg(feature = "standalone")]
mod standalone_api;

use leptos::mount::mount_to_body;
use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use components::ChatView;

fn main() {
    console_error_panic_hook::set_once();
    mount_to_body(App);
}

#[component]
fn App() -> impl IntoView {
    #[cfg(feature = "standalone")]
    {
        standalone_app()
    }
    #[cfg(not(feature = "standalone"))]
    {
        backend_app()
    }
}

#[cfg(not(feature = "standalone"))]
fn backend_app() -> impl IntoView {
    let api_base = api_base_url();
    let (projects, set_projects) = signal(Vec::<String>::new());

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

#[cfg(feature = "standalone")]
fn standalone_app() -> impl IntoView {
    use code_rag_engine::intent::IntentClassifier;
    use components::AuthPanel;
    use std::sync::Arc;

    // Auth state (loaded from localStorage)
    let auth_signal: RwSignal<Option<auth::AuthMethod>> = RwSignal::new(auth::load_auth());
    provide_context(auth_signal);

    // Index + classifier signals (None until loaded)
    let index_signal: RwSignal<Option<Arc<data::ChunkIndex>>> = RwSignal::new(None);
    let classifier_signal: RwSignal<Option<Arc<IntentClassifier>>> = RwSignal::new(None);
    provide_context(index_signal);
    provide_context(classifier_signal);

    let (projects, set_projects) = signal(Vec::<String>::new());
    let (load_error, set_load_error) = signal(Option::<String>::None);
    let (embedder_status, set_embedder_status) = signal("Loading index...");

    // Load index, build classifier, pre-warm embedder
    spawn_local(async move {
        match data::load_index("static/index.json").await {
            Ok(index) => {
                set_projects.set(index.projects.clone());
                let classifier = standalone_api::build_classifier(&index);
                classifier_signal.set(Some(Arc::new(classifier)));
                index_signal.set(Some(Arc::new(index)));

                set_embedder_status.set("Loading AI model...");
                // Pre-warm the embedder (downloads model on first use)
                match embedder::init().await {
                    Ok(()) => set_embedder_status.set("Ready"),
                    Err(e) => {
                        // Non-fatal: embedder will retry on first query
                        web_sys::console::warn_1(&format!("Embedder pre-warm failed: {e}").into());
                        set_embedder_status.set("Ready");
                    }
                }
            }
            Err(e) => {
                set_load_error.set(Some(format!("Failed to load index: {e}")));
            }
        }
    });

    let api_base = api_base_url();
    let index_ready = move || index_signal.get().is_some();

    view! {
        <div class="app">
            <header class="app-header">
                <div class="header-top">
                    <h1>"Code RAG Chat"</h1>
                    <AuthPanel />
                </div>
                <p>"Ask questions about the indexed codebase"</p>
                <Show when=move || {
                    !index_ready() && load_error.get().is_none()
                }>
                    <p class="loading-status">{move || embedder_status.get()}</p>
                </Show>
            </header>

            <Show when=move || load_error.get().is_some()>
                <div class="error-banner">
                    {move || load_error.get().unwrap_or_default()}
                </div>
            </Show>

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

            <Show when=index_ready>
                <ChatView api_base=api_base.clone() />
            </Show>
        </div>
    }
}

/// Determine the API base URL.
fn api_base_url() -> String {
    web_sys::window()
        .and_then(|w| w.location().origin().ok())
        .unwrap_or_else(|| String::from("http://127.0.0.1:3000"))
}
