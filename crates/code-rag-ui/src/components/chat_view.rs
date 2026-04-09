use leptos::html::Textarea;
use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

#[cfg(not(feature = "standalone"))]
use crate::api;
use crate::api::ChatResponse;
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
pub fn ChatView(#[allow(unused_variables)] api_base: String) -> impl IntoView {
    let (messages, set_messages) = signal(Vec::<ChatMessage>::new());
    let (input, set_input) = signal(String::new());
    let (loading, set_loading) = signal(false);
    let textarea_ref = NodeRef::<Textarea>::new();

    // Standalone mode: grab context signals set up by main.rs
    #[cfg(feature = "standalone")]
    let index_signal = use_context::<RwSignal<Option<std::sync::Arc<crate::data::ChunkIndex>>>>()
        .expect("ChunkIndex context missing");
    #[cfg(feature = "standalone")]
    let classifier_signal = use_context::<
        RwSignal<Option<std::sync::Arc<code_rag_engine::intent::IntentClassifier>>>,
    >()
    .expect("IntentClassifier context missing");
    #[cfg(feature = "standalone")]
    let auth_signal = use_context::<RwSignal<Option<crate::auth::AuthMethod>>>()
        .expect("AuthMethod context missing");

    // Stash api_base in a Copy handle so `on_submit` stays Copy in both
    // standalone and non-standalone builds (the latter would otherwise
    // capture a String and need cloning at every call site).
    #[cfg(not(feature = "standalone"))]
    let api_base_submit = StoredValue::new(api_base.clone());

    let on_submit = move || {
        let query = input.get().trim().to_string();
        if query.is_empty() || loading.get() {
            return;
        }

        set_input.set(String::new());
        set_loading.set(true);
        set_messages.update(|msgs| msgs.push(ChatMessage::User(query.clone())));

        #[cfg(feature = "standalone")]
        {
            let _index = index_signal;
            let _classifier = classifier_signal;
            let _auth = auth_signal;
            spawn_local(async move {
                let result = standalone_chat(&query, _index, _classifier, _auth).await;
                match result {
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
                scroll_to_bottom();
            });
        }

        #[cfg(not(feature = "standalone"))]
        {
            let base = api_base_submit.get_value();
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
                scroll_to_bottom();
            });
        }
    };

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
                                let answer_html = md_to_html(&resp.answer);
                                view! {
                                    <div class="message assistant">
                                        <span class="message-label">
                                            "Assistant "
                                            <IntentBadge intent=intent />
                                        </span>
                                        <div class="message-bubble" inner_html=answer_html></div>
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
                            on_submit();
                        }
                    }
                    disabled=move || loading.get()
                />
                <button
                    class="send-btn"
                    on:click=move |_| on_submit()
                    disabled=move || loading.get() || input.get().trim().is_empty()
                >
                    "Send"
                </button>
            </div>
        </div>
    }
}

#[cfg(feature = "standalone")]
async fn standalone_chat(
    query: &str,
    index_signal: RwSignal<Option<std::sync::Arc<crate::data::ChunkIndex>>>,
    classifier_signal: RwSignal<Option<std::sync::Arc<code_rag_engine::intent::IntentClassifier>>>,
    auth_signal: RwSignal<Option<crate::auth::AuthMethod>>,
) -> Result<ChatResponse, String> {
    let embedding = crate::embedder::embed_query(query).await?;

    let index = index_signal.get_untracked().ok_or("Index not loaded")?;
    let classifier = classifier_signal
        .get_untracked()
        .ok_or("Classifier not loaded")?;

    match auth_signal.get_untracked() {
        Some(ref a) if a.is_valid() => {
            crate::standalone_api::send_chat_standalone(query, &embedding, &index, &classifier, a)
                .await
        }
        _ => {
            crate::standalone_api::send_chat_rag_only(query, &embedding, &index, &classifier).await
        }
    }
}

fn md_to_html(md: &str) -> String {
    let parser = pulldown_cmark::Parser::new(md);
    let mut html = String::new();
    pulldown_cmark::html::push_html(&mut html, parser);
    html
}

fn scroll_to_bottom() {
    if let Some(doc) = web_sys::window().and_then(|w| w.document())
        && let Some(el) = doc.query_selector(".chat-container").ok().flatten()
    {
        el.set_scroll_top(el.scroll_height());
    }
}
