//! WASM bridge to the R5 topology renderer (`static/graph.js`) — same
//! global-function pattern as `embedder.rs` / `reranker.rs`.
//!
//! The renderer is a singleton: one graph at a time; `init` tears down any
//! previous instance itself. The click callback crosses JS→Rust as a
//! stringified node (see `viz_data::VizNodeClick`) so the boundary stays one
//! typed function, not an object protocol.

use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(catch, js_name = "__codeRagGraphInit")]
    async fn js_graph_init(
        canvas: &web_sys::HtmlCanvasElement,
        data_json: &str,
        on_node_click: &js_sys::Function,
    ) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(js_name = "__codeRagGraphDestroy")]
    fn js_graph_destroy();

    #[wasm_bindgen(js_name = "__codeRagGraphResize")]
    fn js_graph_resize();

    #[wasm_bindgen(js_name = "__codeRagGraphSetVisible")]
    fn js_graph_set_visible(visible: bool);
}

/// Initialize (or replace) the singleton graph on `canvas` with the raw viz
/// artifact JSON. The caller owns `on_node_click`'s backing `Closure` and must
/// keep it alive until after [`destroy`].
pub async fn init(
    canvas: &web_sys::HtmlCanvasElement,
    data_json: &str,
    on_node_click: &js_sys::Function,
) -> Result<(), String> {
    js_graph_init(canvas, data_json, on_node_click)
        .await
        .map(|_| ())
        .map_err(|e| format!("Graph init failed: {e:?}"))
}

/// Tear down the simulation, listeners, observers and tooltip. No-op when
/// nothing is initialized.
pub fn destroy() {
    js_graph_destroy();
}

/// Re-measure the canvas against its container (call when the tab becomes
/// visible — a hidden canvas measures 0×0).
pub fn resize() {
    js_graph_resize();
}

/// Pause/resume the render loop (tab hidden ↔ shown).
pub fn set_visible(visible: bool) {
    js_graph_set_visible(visible);
}
