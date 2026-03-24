use leptos::prelude::*;

#[component]
pub fn IntentBadge(intent: String) -> impl IntoView {
    view! {
        <span class="intent-badge">{intent}</span>
    }
}
