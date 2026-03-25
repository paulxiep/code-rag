//! Standalone API — runs engine in-browser instead of calling /chat endpoint.

use std::collections::HashMap;

use code_rag_engine::context;
use code_rag_engine::intent::{self, IntentClassifier, QueryIntent, RoutingTable};
use code_rag_engine::retriever;

use crate::api::{ChatResponse, SourceInfo};
use crate::auth::AuthMethod;
use crate::data::ChunkIndex;
use crate::gemini;
use crate::search;

/// Build IntentClassifier from pre-computed prototype embeddings in the index.
pub fn build_classifier(index: &ChunkIndex) -> IntentClassifier {
    let mut prototypes: HashMap<QueryIntent, Vec<Vec<f32>>> = HashMap::new();

    for (key, embeddings) in &index.intent_prototypes {
        let intent = match key.as_str() {
            "overview" => QueryIntent::Overview,
            "implementation" => QueryIntent::Implementation,
            "relationship" => QueryIntent::Relationship,
            "comparison" => QueryIntent::Comparison,
            _ => continue,
        };
        prototypes.insert(intent, embeddings.clone());
    }

    IntentClassifier::from_prototypes(prototypes)
}

/// Run the full RAG pipeline in-browser and return a ChatResponse.
pub async fn send_chat_standalone(
    query: &str,
    query_embedding: &[f32],
    index: &ChunkIndex,
    classifier: &IntentClassifier,
    auth: &AuthMethod,
) -> Result<ChatResponse, String> {
    let routing = RoutingTable::default();

    // Classify intent
    let classification = intent::classify(query_embedding, classifier);
    let config = intent::route(classification.intent, &routing);

    // Brute-force vector search
    let (code_raw, readme_raw, crate_raw, module_doc_raw) =
        search::brute_force_search(query_embedding, index, &config);

    // Score and build retrieval result
    let result =
        retriever::to_retrieval_result(code_raw, readme_raw, crate_raw, module_doc_raw, classification.intent);

    // Build context and prompt
    let ctx = context::build_context(&result);
    let prompt = context::build_prompt(query, &ctx);

    // Call Gemini API
    let answer = gemini::generate(&prompt, auth).await?;

    // Build source list (same format as backend API response)
    let mut sources: Vec<SourceInfo> = Vec::new();
    for s in &result.code_chunks {
        sources.push(SourceInfo {
            chunk_type: "code".into(),
            path: s.chunk.file_path.clone(),
            label: s.chunk.identifier.clone(),
            project: s.chunk.project_name.clone(),
            relevance: s.score,
            relevance_pct: (s.score * 100.0).round() as u8,
            line: s.chunk.start_line,
        });
    }
    for s in &result.readme_chunks {
        sources.push(SourceInfo {
            chunk_type: "readme".into(),
            path: s.chunk.file_path.clone(),
            label: s.chunk.project_name.clone(),
            project: s.chunk.project_name.clone(),
            relevance: s.score,
            relevance_pct: (s.score * 100.0).round() as u8,
            line: 0,
        });
    }
    for s in &result.crate_chunks {
        sources.push(SourceInfo {
            chunk_type: "crate".into(),
            path: s.chunk.crate_path.clone(),
            label: s.chunk.crate_name.clone(),
            project: s.chunk.project_name.clone(),
            relevance: s.score,
            relevance_pct: (s.score * 100.0).round() as u8,
            line: 0,
        });
    }
    for s in &result.module_doc_chunks {
        sources.push(SourceInfo {
            chunk_type: "module_doc".into(),
            path: s.chunk.file_path.clone(),
            label: s.chunk.module_name.clone(),
            project: s.chunk.project_name.clone(),
            relevance: s.score,
            relevance_pct: (s.score * 100.0).round() as u8,
            line: 0,
        });
    }
    sources.sort_by(|a, b| {
        b.relevance
            .partial_cmp(&a.relevance)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let intent_str = serde_json::to_string(&classification.intent)
        .unwrap_or_else(|_| "\"unknown\"".to_string())
        .trim_matches('"')
        .to_string();

    Ok(ChatResponse {
        answer,
        sources,
        intent: intent_str,
    })
}
