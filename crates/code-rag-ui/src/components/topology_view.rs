//! R5: interactive topology view — emergent communities rendered as a
//! force-directed graph (d3-force via `static/graph.js`), one project at a
//! time, with click-a-node → run a code-rag query.

use std::collections::HashMap;

use leptos::html::Canvas;
use leptos::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::Closure;
use wasm_bindgen_futures::spawn_local;

use crate::viz_data::{self, VizFetchError, VizMeta, VizNodeClick};
use crate::{ActiveTab, PendingChat, PendingQuery, SelectedProject, graph_bridge};

/// Fixed palette slots; community ids past the last slot render as "Other".
/// Never wraps — two communities must never share a hue (an honest legend).
const LEGEND_SLOTS: u32 = 8;

#[derive(Clone, PartialEq)]
enum VizState {
    Idle,
    Loading,
    Ready,
    /// 404 — the deployed bundle has no artifact for this project.
    Missing,
    Error(String),
}

/// CSS color for a community id (slots 1..=8, then the neutral "Other").
fn community_color(id: u32) -> String {
    if id < LEGEND_SLOTS {
        format!("var(--viz-c{})", id + 1)
    } else {
        "var(--viz-other)".to_string()
    }
}

/// The chat query a node click produces. Phrasing steers the intent
/// classifier toward the arm that fits the node: definitions → the
/// relationship/call-graph arm, containers → the overview arm (folder/file
/// chunks). Naming the project disambiguates generic identifiers (`Player`
/// exists in several projects) for both the reranker and the LLM; the exact
/// clicked chunk additionally rides along as the retrieval anchor.
fn node_query(node: &VizNodeClick) -> String {
    let name = if node.label.is_empty() {
        node.id.as_str()
    } else {
        node.label.as_str()
    };
    // File paths are project-prefixed (`7_wonders/elements.py`).
    let project = node.file.split('/').next().unwrap_or("");
    if node.kind == "file" {
        if project.is_empty() {
            format!("What is the role of `{name}` in the architecture of this project?")
        } else {
            format!("What is the role of `{name}` in the architecture of the `{project}` project?")
        }
    } else {
        let basename = node.file.rsplit('/').next().unwrap_or(&node.file);
        if basename.is_empty() || project.is_empty() {
            format!("What does `{name}` do and what depends on it?")
        } else {
            format!(
                "What does `{name}` in `{basename}` (`{project}` project) do and what depends on it?"
            )
        }
    }
}

/// The topology tab. Mounted lazily (first activation) by `standalone_app`,
/// then kept alive across tab switches so the layout survives.
#[component]
pub fn TopologyView() -> impl IntoView {
    let tab = use_context::<RwSignal<ActiveTab>>().expect("ActiveTab context missing");
    let pending = use_context::<PendingQuery>().expect("PendingQuery context missing");
    // Selection lives app-wide: the top projects bar (main.rs) is the single
    // selector, visible on both tabs.
    let selected = use_context::<SelectedProject>()
        .expect("SelectedProject context missing")
        .0;
    let state: RwSignal<VizState> = RwSignal::new(VizState::Idle);
    let meta: RwSignal<Option<VizMeta>> = RwSignal::new(None);
    // Raw artifact JSON per project — fetched once per session, handed to
    // graph.js on (re-)init.
    let cache: StoredValue<HashMap<String, (String, VizMeta)>> = StoredValue::new(HashMap::new());
    let canvas_ref = NodeRef::<Canvas>::new();
    // The JS click callback's backing closure — must outlive the graph, so it
    // lives here and is dropped on cleanup (after destroy). `Closure` is !Send
    // → local storage.
    let click_closure: StoredValue<Option<Closure<dyn Fn(String)>>, LocalStorage> =
        StoredValue::new_local(None);

    // Fetch (or serve cached) on selection change.
    Effect::new(move |_| {
        let Some(project) = selected.get() else {
            return;
        };
        if let Some((_json, m)) = cache.with_value(|c| c.get(&project).cloned()) {
            meta.set(Some(m));
            state.set(VizState::Ready);
            return;
        }
        state.set(VizState::Loading);
        meta.set(None);
        spawn_local(async move {
            match viz_data::fetch_viz(&project).await {
                Ok((json, m)) => {
                    cache.update_value(|c| {
                        c.insert(project.clone(), (json, m.clone()));
                    });
                    meta.set(Some(m));
                    state.set(VizState::Ready);
                }
                Err(VizFetchError::Missing) => state.set(VizState::Missing),
                Err(VizFetchError::Other(e)) => state.set(VizState::Error(e)),
            }
        });
    });

    // (Re-)initialize the renderer whenever the artifact for the selected
    // project becomes ready and the canvas is mounted.
    Effect::new(move |_| {
        if state.get() != VizState::Ready {
            return;
        }
        let Some(canvas) = canvas_ref.get() else {
            return;
        };
        let Some(project) = selected.get() else {
            return;
        };
        let Some((json, _)) = cache.with_value(|c| c.get(&project).cloned()) else {
            return;
        };

        let closure = Closure::wrap(Box::new(move |node_json: String| {
            if let Ok(node) = serde_json::from_str::<VizNodeClick>(&node_json) {
                pending.0.set(Some(PendingChat {
                    query: node_query(&node),
                    // The clicked chunk itself — guaranteed into the context,
                    // no identifier-resolution lottery.
                    anchor_chunk_id: Some(node.id.clone()),
                }));
                tab.set(ActiveTab::Chat);
            }
        }) as Box<dyn Fn(String)>);
        let callback: js_sys::Function =
            closure.as_ref().unchecked_ref::<js_sys::Function>().clone();
        // Replace (and drop) any previous closure — its graph instance is torn
        // down by the init below before the new one attaches.
        click_closure.set_value(Some(closure));

        spawn_local(async move {
            // Yield once so the just-unhidden canvas has real layout size
            // before graph.js measures it.
            gloo_timers::future::TimeoutFuture::new(0).await;
            if let Err(e) = graph_bridge::init(&canvas, &json, &callback).await {
                state.set(VizState::Error(e));
            }
        });
    });

    // Pause the render loop while the tab is hidden; re-measure on return.
    Effect::new(move |_| {
        let active = tab.get() == ActiveTab::Topology;
        if active {
            graph_bridge::resize();
        }
        graph_bridge::set_visible(active);
    });

    on_cleanup(move || {
        graph_bridge::destroy();
        click_closure.set_value(None);
    });

    let status_line = move || match state.get() {
        VizState::Idle | VizState::Ready => None,
        VizState::Loading => Some(
            view! {
                <p class="loading-status">"Loading topology…"</p>
            }
            .into_any(),
        ),
        VizState::Missing => Some(
            view! {
                <p class="viz-missing">"No topology data for this project."</p>
            }
            .into_any(),
        ),
        VizState::Error(e) => Some(view! { <div class="error-banner">{e}</div> }.into_any()),
    };

    let legend = move || {
        meta.get().map(|m| {
            let shown: Vec<_> = m
                .communities
                .iter()
                .filter(|c| c.id < LEGEND_SLOTS)
                .cloned()
                .collect();
            let other_count = m.communities.len().saturating_sub(shown.len());
            let truncated_note = m.truncated.then(|| {
                view! {
                    <p class="viz-note">
                        {format!(
                            "Showing the most-connected part of a {} node / {} edge topology.",
                            m.node_total, m.edge_total
                        )}
                    </p>
                }
            });
            view! {
                <div class="viz-legend">
                    <div class="viz-legend-communities">
                        {shown
                            .into_iter()
                            .map(|c| {
                                let title = if c.dir.is_empty() {
                                    format!("{} members · cohesion {:.2}", c.size, c.cohesion)
                                } else {
                                    format!(
                                        "{} members · cohesion {:.2} — {}",
                                        c.size, c.cohesion, c.dir
                                    )
                                };
                                view! {
                                    <span class="viz-chip" title=title>
                                        <span
                                            class="viz-swatch"
                                            style:background=community_color(c.id)
                                        ></span>
                                        {if c.label.is_empty() {
                                            format!("community {}", c.id)
                                        } else {
                                            c.label.clone()
                                        }}
                                    </span>
                                }
                            })
                            .collect_view()}
                        <Show when=move || { other_count > 0 }>
                            <span class="viz-chip">
                                <span
                                    class="viz-swatch"
                                    style:background="var(--viz-other)"
                                ></span>
                                {format!("other ({other_count} more)")}
                            </span>
                        </Show>
                    </div>
                    <div class="viz-legend-edges">
                        <span class="viz-chip">
                            <svg width="26" height="6" aria-hidden="true">
                                <line x1="0" y1="3" x2="26" y2="3" stroke="currentColor" stroke-width="2"/>
                            </svg>
                            "calls"
                        </span>
                        <span class="viz-chip">
                            <svg width="26" height="6" aria-hidden="true">
                                <line x1="0" y1="3" x2="26" y2="3" stroke="currentColor" stroke-width="2" stroke-dasharray="6,3"/>
                            </svg>
                            "imports"
                        </span>
                        <span class="viz-chip">
                            <svg width="26" height="6" aria-hidden="true">
                                <line x1="0" y1="3" x2="26" y2="3" stroke="currentColor" stroke-width="2" stroke-dasharray="2,3"/>
                            </svg>
                            "type links"
                        </span>
                        <span class="viz-chip">
                            <svg width="26" height="6" aria-hidden="true">
                                <line x1="0" y1="3" x2="26" y2="3" stroke="currentColor" stroke-width="2" stroke-dasharray="1,3"/>
                            </svg>
                            "contains"
                        </span>
                        <span class="viz-chip viz-chip-note">
                            "node size = connections · faded = inferred"
                        </span>
                    </div>
                    {truncated_note}
                </div>
            }
        })
    };

    view! {
        <div class="topology-view">
            {status_line}
            <div class="viz-canvas-wrap" class:hidden=move || state.get() != VizState::Ready>
                <canvas class="viz-canvas" node_ref=canvas_ref></canvas>
            </div>
            {legend}
        </div>
    }
}
