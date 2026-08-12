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
mod graph_bridge;
#[cfg(feature = "standalone")]
mod reranker;
#[cfg(feature = "standalone")]
mod search;
#[cfg(feature = "standalone")]
mod standalone_api;
#[cfg(feature = "standalone")]
mod text_search;
#[cfg(feature = "standalone")]
mod viz_data;

use leptos::mount::mount_to_body;
use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use components::ChatView;

/// Which top-level view is active (standalone demo only).
#[cfg(feature = "standalone")]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ActiveTab {
    Chat,
    Topology,
}

/// Context newtype: a query queued by the topology view (click-a-node) for
/// `ChatView` to pick up and submit. Newtype so the context can never collide
/// with another `RwSignal<Option<String>>`.
#[cfg(feature = "standalone")]
#[derive(Clone, Copy)]
pub struct PendingQuery(pub RwSignal<Option<PendingChat>>);

/// A chat request queued by another view (topology click-a-node). Carries the
/// clicked chunk's id so retrieval can guarantee that exact chunk reaches the
/// context — identifier resolution alone is unreliable for generic names that
/// exist in several projects (`Player`, `Config`, …).
#[cfg(feature = "standalone")]
#[derive(Clone)]
pub struct PendingChat {
    pub query: String,
    pub anchor_chunk_id: Option<String>,
}

/// R5: the selected project, shared app-wide. The top projects bar is the
/// single selector (visible on both tabs); today only the topology view
/// consumes the selection — chat searches the whole corpus regardless.
#[cfg(feature = "standalone")]
#[derive(Clone, Copy)]
pub struct SelectedProject(pub RwSignal<Option<String>>);

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
    use components::{AuthPanel, TopologyView};
    use std::sync::Arc;

    // Auth state (loaded from localStorage)
    let auth_signal: RwSignal<Option<auth::AuthMethod>> = RwSignal::new(auth::load_auth());
    provide_context(auth_signal);

    // Index + classifier signals (None until loaded)
    let index_signal: RwSignal<Option<Arc<data::ChunkIndex>>> = RwSignal::new(None);
    let classifier_signal: RwSignal<Option<Arc<IntentClassifier>>> = RwSignal::new(None);
    provide_context(index_signal);
    provide_context(classifier_signal);

    // R5: active tab + the topology→chat pending-query channel.
    let tab: RwSignal<ActiveTab> = RwSignal::new(ActiveTab::Chat);
    provide_context(tab);
    provide_context(PendingQuery(RwSignal::new(None)));
    // One selection, one bar: the top projects bar drives the topology view.
    let selected_project = SelectedProject(RwSignal::new(None));
    provide_context(selected_project);
    // Latches true on first activation so users who never open the topology
    // tab never pay its artifact fetch / simulation cost.
    let topology_opened = RwSignal::new(false);

    let (projects, set_projects) = signal(Vec::<String>::new());
    let (load_error, set_load_error) = signal(Option::<String>::None);
    let (embedder_status, set_embedder_status) = signal("Loading index...");

    // Load index, build classifier, pre-warm embedder
    spawn_local(async move {
        match data::load_index("static/index.json").await {
            Ok(index) => {
                set_projects.set(index.projects.clone());
                // Default selection: first indexed project.
                if let Some(first) = index.projects.first().cloned() {
                    selected_project.0.set(Some(first));
                }
                let classifier = standalone_api::build_classifier(&index);
                classifier_signal.set(Some(Arc::new(classifier)));
                index_signal.set(Some(Arc::new(index)));

                set_embedder_status.set("Loading AI models...");
                // Pre-warm the embedder (downloads model on first use)
                match embedder::init().await {
                    Ok(()) => {}
                    Err(e) => {
                        web_sys::console::warn_1(&format!("Embedder pre-warm failed: {e}").into());
                    }
                }
                // Pre-warm the reranker (non-fatal on failure)
                match reranker::init().await {
                    Ok(()) => {}
                    Err(e) => {
                        web_sys::console::warn_1(&format!("Reranker pre-warm failed: {e}").into());
                    }
                }
                set_embedder_status.set("Ready");
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

            // The single project bar: shown on both tabs, drives the
            // topology view's selection (chat searches all projects).
            <Show when=move || !projects.get().is_empty()>
                <div class="projects-bar">
                    <For
                        each=move || projects.get()
                        key=|p| p.clone()
                        let:project
                    >
                        {
                            let name = project.clone();
                            let is_active = {
                                let name = name.clone();
                                move || {
                                    selected_project.0.get().as_deref()
                                        == Some(name.as_str())
                                }
                            };
                            view! {
                                <button
                                    class="project-tag"
                                    class:active=is_active
                                    on:click=move |_| {
                                        selected_project.0.set(Some(name.clone()))
                                    }
                                >
                                    {project.clone()}
                                </button>
                            }
                        }
                    </For>
                </div>
            </Show>

            <Show when=index_ready>
                <div class="tab-strip" role="tablist">
                    <button
                        class="tab-btn"
                        role="tab"
                        class:active=move || tab.get() == ActiveTab::Chat
                        attr:aria-selected=move || (tab.get() == ActiveTab::Chat).to_string()
                        on:click=move |_| tab.set(ActiveTab::Chat)
                    >
                        "Chat"
                    </button>
                    <button
                        class="tab-btn"
                        role="tab"
                        class:active=move || tab.get() == ActiveTab::Topology
                        attr:aria-selected=move || (tab.get() == ActiveTab::Topology).to_string()
                        on:click=move |_| {
                            topology_opened.set(true);
                            tab.set(ActiveTab::Topology);
                        }
                    >
                        "Topology"
                    </button>
                </div>
                // Both panels stay mounted (chat history + graph layout are
                // component-local state); tabs only toggle visibility.
                <div class="tab-panel" class:hidden=move || tab.get() != ActiveTab::Chat>
                    <ChatView api_base=api_base.clone() />
                </div>
                <div class="tab-panel" class:hidden=move || tab.get() != ActiveTab::Topology>
                    <Show when=move || topology_opened.get()>
                        <TopologyView />
                    </Show>
                </div>
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
