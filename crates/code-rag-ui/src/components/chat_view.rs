use leptos::prelude::*;
use leptos::html::Textarea;
use wasm_bindgen_futures::spawn_local;

use crate::api::{self, ChatResponse};
use crate::components::{IntentBadge, SourcesPanel};

/// A single message in the chat history.
#[derive(Clone)]
enum ChatMessage {
    User(String),
    Assistant(AssistantMessage),
    Error(String),
}

#[derive(Clone)]
struct AssistantMessage {
    response: ChatResponse,
}

#[component]
pub fn ChatView(api_base: String) -> impl IntoView {
    let (messages, set_messages) = signal(Vec::<ChatMessage>::new());
    let (input, set_input) = signal(String::new());
    let (loading, set_loading) = signal(false);
    let textarea_ref = NodeRef::<Textarea>::new();

    let api_base_submit = api_base.clone();
    let on_submit = move || {
        let query = input.get().trim().to_string();
        if query.is_empty() || loading.get() {
            return;
        }

        set_input.set(String::new());
        set_loading.set(true);

        // Add user message
        set_messages.update(|msgs| msgs.push(ChatMessage::User(query.clone())));

        let base = api_base_submit.clone();
        spawn_local(async move {
            match api::send_chat(&base, &query).await {
                Ok(response) => {
                    set_messages.update(|msgs| {
                        msgs.push(ChatMessage::Assistant(AssistantMessage { response }));
                    });
                }
                Err(e) => {
                    set_messages.update(|msgs| msgs.push(ChatMessage::Error(e)));
                }
            }
            set_loading.set(false);

            // Scroll to bottom after response
            if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
                if let Some(el) = doc.query_selector(".chat-container").ok().flatten() {
                    el.set_scroll_top(el.scroll_height());
                }
            }
        });
    };

    let on_submit_click = on_submit.clone();
    let on_submit_key = on_submit.clone();

    view! {
        <div class="chat-container">
            <Show
                when=move || !messages.get().is_empty()
                fallback=|| {
                    view! {
                        <div class="empty-state">
                            <h2>"Ask about the codebase"</h2>
                            <p>"Try: \"How does the ingestion pipeline work?\" or \"What is the intent classifier?\""</p>
                        </div>
                    }
                }
            >
                <For
                    each=move || {
                        messages
                            .get()
                            .into_iter()
                            .enumerate()
                            .collect::<Vec<_>>()
                    }
                    key=|(i, _)| *i
                    let:entry
                >
                    {move || {
                        let (_, msg) = entry.clone();
                        match msg {
                            ChatMessage::User(text) => {
                                view! {
                                    <div class="message user">
                                        <span class="message-label">"You"</span>
                                        <div class="message-bubble">{text}</div>
                                    </div>
                                }
                                    .into_any()
                            }
                            ChatMessage::Assistant(am) => {
                                let resp = am.response;
                                let intent = resp.intent.clone();
                                let sources = resp.sources.clone();
                                view! {
                                    <div class="message assistant">
                                        <span class="message-label">
                                            "Assistant "
                                            <IntentBadge intent=intent />
                                        </span>
                                        <div class="message-bubble" inner_html=resp.answer></div>
                                        <SourcesPanel sources=sources />
                                    </div>
                                }
                                    .into_any()
                            }
                            ChatMessage::Error(e) => {
                                view! {
                                    <div class="message assistant">
                                        <span class="message-label">"Error"</span>
                                        <div class="message-bubble">{e}</div>
                                    </div>
                                }
                                    .into_any()
                            }
                        }
                    }}
                </For>
                <Show when=move || loading.get()>
                    <div class="message assistant">
                        <span class="message-label">"Assistant"</span>
                        <div class="message-bubble">
                            <span class="loading-dots">"Thinking"</span>
                        </div>
                    </div>
                </Show>
            </Show>
        </div>

        <div class="chat-input">
            <div class="input-row">
                <textarea
                    node_ref=textarea_ref
                    rows="2"
                    placeholder="Ask about the codebase..."
                    prop:value=move || input.get()
                    on:input=move |ev| {
                        let target = event_target::<web_sys::HtmlTextAreaElement>(&ev);
                        set_input.set(target.value());
                    }
                    on:keydown=move |ev: web_sys::KeyboardEvent| {
                        if ev.key() == "Enter" && !ev.shift_key() {
                            ev.prevent_default();
                            on_submit_key();
                        }
                    }
                    disabled=move || loading.get()
                />
                <button
                    class="send-btn"
                    on:click=move |_| on_submit_click()
                    disabled=move || loading.get() || input.get().trim().is_empty()
                >
                    "Send"
                </button>
            </div>
        </div>
    }
}
