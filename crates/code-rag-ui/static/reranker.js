// Cross-encoder reranking via transformers.js (ms-marco-MiniLM-L-6-v2, ~22MB quantized)
// Lazy-loads the model on first use; cached in IndexedDB after first download.
// Must use AutoTokenizer + AutoModelForSequenceClassification with text_pair,
// NOT pipeline('text-classification') which doesn't handle cross-encoder pairs.

let rerankerTokenizer = null;
let rerankerModel = null;

async function loadReranker() {
    if (rerankerTokenizer && rerankerModel) return;
    const { AutoTokenizer, AutoModelForSequenceClassification } = await import(
        "https://cdn.jsdelivr.net/npm/@huggingface/transformers@3.8.1"
    );
    rerankerTokenizer = await AutoTokenizer.from_pretrained(
        "Xenova/ms-marco-MiniLM-L-6-v2"
    );
    rerankerModel = await AutoModelForSequenceClassification.from_pretrained(
        "Xenova/ms-marco-MiniLM-L-6-v2",
        { dtype: "q8" }
    );
}

window.__codeRagRerank = async function (query, documents) {
    await loadReranker();

    const scores = [];
    for (let i = 0; i < documents.length; i++) {
        const inputs = rerankerTokenizer(query, {
            text_pair: documents[i],
            padding: true,
            truncation: true,
        });
        const output = await rerankerModel(inputs);
        scores.push({ index: i, score: output.logits.data[0] });
    }
    scores.sort((a, b) => b.score - a.score);
    return scores;
};

window.__codeRagInitReranker = async function () {
    await loadReranker();
    return true;
};
