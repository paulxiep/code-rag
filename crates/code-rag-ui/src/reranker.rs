//! WASM reranker bridge — calls transformers.js cross-encoder via JS interop.

use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(catch, js_name = "__codeRagRerank")]
    async fn js_rerank(query: &str, documents: JsValue) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(catch, js_name = "__codeRagInitReranker")]
    async fn js_init_reranker() -> Result<JsValue, JsValue>;
}

/// Rerank documents via transformers.js cross-encoder.
/// Returns Vec<(index, logit_score)> sorted by score descending.
pub async fn rerank(query: &str, documents: Vec<String>) -> Result<Vec<(usize, f32)>, String> {
    if documents.is_empty() {
        return Ok(Vec::new());
    }

    let js_docs =
        serde_wasm_bindgen::to_value(&documents).map_err(|e| format!("serialize: {e}"))?;

    let result = js_rerank(query, js_docs)
        .await
        .map_err(|e| format!("reranking failed: {e:?}"))?;

    let array = js_sys::Array::from(&result);
    let mut scores = Vec::with_capacity(array.length() as usize);
    for i in 0..array.length() {
        let entry = array.get(i);
        let index = js_sys::Reflect::get(&entry, &"index".into())
            .unwrap_or(JsValue::from(0))
            .as_f64()
            .unwrap_or(0.0) as usize;
        let score = js_sys::Reflect::get(&entry, &"score".into())
            .unwrap_or(JsValue::from(0.0))
            .as_f64()
            .unwrap_or(0.0) as f32;
        scores.push((index, score));
    }
    Ok(scores)
}

/// Pre-load the reranker model (call during init for faster first query).
pub async fn init() -> Result<(), String> {
    js_init_reranker()
        .await
        .map_err(|e| format!("Reranker init failed: {e:?}"))?;
    Ok(())
}
