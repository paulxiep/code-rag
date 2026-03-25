// Query embedding via transformers.js (BGE-small-en-v1.5, 384-dim)
// Lazy-loads the model on first use; cached in IndexedDB after first download.

let extractor = null;

async function loadPipeline() {
    if (extractor) return extractor;
    const { pipeline } = await import(
        "https://cdn.jsdelivr.net/npm/@huggingface/transformers@3.8.1"
    );
    extractor = await pipeline("feature-extraction", "Xenova/bge-small-en-v1.5", {
        dtype: "fp32",
    });
    return extractor;
}

window.__codeRagEmbedQuery = async function (text) {
    const pipe = await loadPipeline();
    const result = await pipe(text, { pooling: "cls", normalize: true });
    return Array.from(result.data);
};

window.__codeRagInitEmbedder = async function () {
    await loadPipeline();
    return true;
};
