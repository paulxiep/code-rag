//! WASM embedder bridge — calls transformers.js via JS interop.

use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(catch, js_name = "__codeRagEmbedQuery")]
    async fn js_embed_query(text: &str) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(catch, js_name = "__codeRagInitEmbedder")]
    async fn js_init_embedder() -> Result<JsValue, JsValue>;
}

/// Embed a query string, returning a 384-dim f32 vector.
/// On first call, downloads the model (~33 MB, cached in IndexedDB).
pub async fn embed_query(text: &str) -> Result<Vec<f32>, String> {
    let result = js_embed_query(text)
        .await
        .map_err(|e| format!("Embedding failed: {e:?}"))?;

    let array = js_sys::Array::from(&result);
    let embedding: Vec<f32> = (0..array.length())
        .map(|i| array.get(i).as_f64().unwrap_or(0.0) as f32)
        .collect();

    Ok(embedding)
}

/// Pre-load the embedding model (call during init for faster first query).
pub async fn init() -> Result<(), String> {
    js_init_embedder()
        .await
        .map_err(|e| format!("Embedder init failed: {e:?}"))?;
    Ok(())
}
