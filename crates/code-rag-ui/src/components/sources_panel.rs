use leptos::prelude::*;

use crate::api::SourceInfo;

#[component]
pub fn SourcesPanel(sources: Vec<SourceInfo>) -> impl IntoView {
    let (expanded, set_expanded) = signal(false);
    let count = sources.len();

    view! {
        <div class="sources-panel">
            <button
                class="sources-toggle"
                on:click=move |_| set_expanded.set(!expanded.get())
            >
                <span>{move || if expanded.get() { "\u{25BC}" } else { "\u{25B6}" }}</span>
                {format!("{count} sources")}
            </button>
            <Show when=move || expanded.get()>
                <div class="sources-list">
                    {sources
                        .iter()
                        .map(|s| {
                            let path_display = if s.line > 0 {
                                format!("{}:{}", s.path, s.line)
                            } else {
                                s.path.clone()
                            };
                            view! {
                                <div class="source-card">
                                    <div class="source-header">
                                        <span class="source-label">{s.label.clone()}</span>
                                        <span class="source-type">{s.chunk_type.clone()}</span>
                                        <span class="source-relevance">
                                            {format!("{}%", s.relevance_pct)}
                                        </span>
                                    </div>
                                    <div class="source-path">{path_display}</div>
                                </div>
                            }
                        })
                        .collect::<Vec<_>>()}
                </div>
            </Show>
        </div>
    }
}
