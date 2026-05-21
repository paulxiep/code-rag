//! Caravan RPC seam declarations for the storage-and-LLM layer.
//!
//! At B0p, each `#[wagon]` trait is the M2 visual surface (the macro is
//! currently identity). Concrete impls live alongside the existing
//! `Embedder`/`Reranker`/`VectorStore`/LLM types in their respective files and
//! get registered with the Caravan RPC SDK at process startup via
//! `caravan_rpc::provide::<dyn Seam>(Arc::new(impl_))`. Call sites use
//! `caravan_rpc::client::<dyn Seam>().method(...)`; when `CARAVAN_RPC_PEERS`
//! is unset the call is a direct method invocation on the registered impl.
//!
//! At M2 the `#[wagon]` macro will expand into wire-codec / server-adapter /
//! client-adapter code without touching the trait surface declared here.
//!
//! Notes on shape:
//! * `Embedder` + `Reranker` are sync (fastembed is sync; the inner `!Sync`
//!   model state moves into the impl via `std::sync::Mutex`).
//! * `VectorReader` is async (LanceDB is async). It carries READS only —
//!   writes stay on the concrete `VectorStore` because code-raptor's ingest
//!   path doesn't dispatch them as RPC.
//! * `LlmClient` is async; its trait returns `LlmError` rather than the
//!   chat-crate `EngineError` so it can live next to the other seams.

use async_trait::async_trait;
use caravan_rpc::wagon;
use thiserror::Error;

use crate::embedder::EmbedError;
use crate::reranker::RerankError;
use crate::vector_store::StoreError;

// LLM seam error (chat-side `EngineError` `#[from]`-wraps this).
#[derive(Error, Debug, serde::Serialize, serde::Deserialize)]
pub enum LlmError {
    #[error("LLM generation failed: {0}")]
    Generation(String),
}

/// Wire-side reranker result. Mirrors `fastembed::RerankResult`'s shape but
/// carries serde derives so the seam method can be encoded as JSON across
/// the HTTP boundary. The `From<fastembed::RerankResult>` conversion at the
/// impl boundary keeps the migration path for call sites transparent —
/// `rr.score` and `rr.index` access stays untouched.
///
/// `document` is preserved as `Option<String>`; `MsMarcoRerankerImpl` passes
/// `false` for fastembed's `return_documents` flag, so `document` is `None`
/// end-to-end today. Kept in the wire shape so a future caller can flip the
/// flag without another wire migration.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RerankResult {
    pub document: Option<String>,
    pub score: f32,
    pub index: usize,
}

impl From<fastembed::RerankResult> for RerankResult {
    fn from(r: fastembed::RerankResult) -> Self {
        Self {
            document: r.document,
            score: r.score,
            index: r.index,
        }
    }
}

// ---------- Embedder ----------

/// Embed text into dense vectors.
///
/// `&self` only: the fastembed model is `!Sync`, so impls must use interior
/// mutability (`Mutex<TextEmbedding>` in the concrete impl). M2 will dispatch
/// each method as a `POST /_caravan/rpc/Embedder/<method>`; the interior lock
/// disappears in HTTP/Lambda mode (each request gets a fresh inproc handle on
/// the remote side).
#[wagon]
pub trait Embedder: Send + Sync {
    fn embed_one(&self, text: &str) -> Result<Vec<f32>, EmbedError>;
    fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError>;
    fn dimension(&self) -> usize;
}

// ---------- Reranker ----------

/// Cross-encoder reranker. Optional seam — the chat target may run without it
/// (config flag); callers should reach for it via `try_client::<dyn Reranker>()`.
///
/// M5: promoted from `#[wagon(identity)]` to full HTTP codegen via the local
/// `RerankResult` wire shim (defined above with serde derives + a `From<
/// fastembed::RerankResult>` conversion). The impl in `reranker.rs` converts
/// at the return boundary.
#[wagon]
pub trait Reranker: Send + Sync {
    /// Documents are passed by value because the cross-encoder consumes them;
    /// callers must already materialize the candidate string set.
    fn rerank(&self, query: &str, documents: Vec<String>)
    -> Result<Vec<RerankResult>, RerankError>;
}

// ---------- VectorReader ----------

/// Read-side over the LanceDB-backed vector store. Writes (`upsert_*`,
/// `delete_*`, `create_fts_indices`) stay on the concrete `VectorStore` —
/// they're only exercised by code-raptor's ingest path, which doesn't go
/// through the RPC seam.
///
/// At M5 the dev plan splits the call-edges graph (`get_all_edges`) out of
/// the vector seam into its own resource group; for B0p it remains here so
/// existing call sites don't have to change shape twice.
#[wagon]
#[async_trait]
pub trait VectorReader: Send + Sync {
    // ---- pure vector search ----
    async fn search_code(
        &self,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<(code_rag_types::CodeChunk, f32)>, StoreError>;

    async fn search_code_signatures(
        &self,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<(code_rag_types::CodeChunk, f32)>, StoreError>;

    async fn search_readme(
        &self,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<(code_rag_types::ReadmeChunk, f32)>, StoreError>;

    async fn search_crates(
        &self,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<(code_rag_types::CrateChunk, f32)>, StoreError>;

    async fn search_module_docs(
        &self,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<(code_rag_types::ModuleDocChunk, f32)>, StoreError>;

    async fn search_folders(
        &self,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<(code_rag_types::FolderChunk, f32)>, StoreError>;

    async fn search_files(
        &self,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<(code_rag_types::FileChunk, f32)>, StoreError>;

    // ---- hybrid (vector + FTS) search ----
    async fn hybrid_search_code(
        &self,
        query_text: &str,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<(code_rag_types::CodeChunk, f32)>, StoreError>;

    async fn hybrid_search_readme(
        &self,
        query_text: &str,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<(code_rag_types::ReadmeChunk, f32)>, StoreError>;

    async fn hybrid_search_crates(
        &self,
        query_text: &str,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<(code_rag_types::CrateChunk, f32)>, StoreError>;

    async fn hybrid_search_module_docs(
        &self,
        query_text: &str,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<(code_rag_types::ModuleDocChunk, f32)>, StoreError>;

    async fn hybrid_search_folders(
        &self,
        query_text: &str,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<(code_rag_types::FolderChunk, f32)>, StoreError>;

    async fn hybrid_search_files(
        &self,
        query_text: &str,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<(code_rag_types::FileChunk, f32)>, StoreError>;

    // ---- catalog ----
    async fn list_projects(&self) -> Result<Vec<String>, StoreError>;

    // ---- by-id lookup ----
    async fn get_chunks_by_ids(
        &self,
        chunk_ids: &[String],
    ) -> Result<Vec<code_rag_types::CodeChunk>, StoreError>;

    // ---- graph (M5 will move this to a separate resource) ----
    async fn get_all_edges(
        &self,
        project_name: &str,
    ) -> Result<Vec<code_rag_types::CallEdge>, StoreError>;

    async fn get_callers(
        &self,
        callee_chunk_id: &str,
        project: Option<&str>,
    ) -> Result<Vec<code_rag_types::CallEdge>, StoreError>;

    async fn get_callees(
        &self,
        caller_chunk_id: &str,
        project: Option<&str>,
    ) -> Result<Vec<code_rag_types::CallEdge>, StoreError>;
}

// ---------- VectorWriter ----------

/// Write-side over the LanceDB-backed vector store. Companion to
/// [`VectorReader`]. M5 split: ingest-time mutations live here; queries
/// stay on `VectorReader`. The concrete `VectorStore` implements both.
///
/// `#[wagon(identity)]` because writes are inproc-only by design — the
/// dev plan keeps ingest (code-raptor) running as a one-shot batch on
/// the same node as the store. Promoting to full HTTP codegen is M4-cloud
/// / Phase 2 work (cloud-managed vector DB). Identity-marked traits don't
/// honor mode flips, which is the intended Phase 1 behavior here.
#[wagon(identity)]
#[async_trait]
pub trait VectorWriter: Send + Sync {
    // ---- upserts (typed chunk + embedding batches) ----
    async fn upsert_code_chunks(
        &self,
        chunks: &[code_rag_types::CodeChunk],
        embeddings: Vec<Vec<f32>>,
        signature_embeddings: Vec<Option<Vec<f32>>>,
    ) -> Result<usize, StoreError>;

    async fn upsert_readme_chunks(
        &self,
        chunks: &[code_rag_types::ReadmeChunk],
        embeddings: Vec<Vec<f32>>,
    ) -> Result<usize, StoreError>;

    async fn upsert_crate_chunks(
        &self,
        chunks: &[code_rag_types::CrateChunk],
        embeddings: Vec<Vec<f32>>,
    ) -> Result<usize, StoreError>;

    async fn upsert_module_doc_chunks(
        &self,
        chunks: &[code_rag_types::ModuleDocChunk],
        embeddings: Vec<Vec<f32>>,
    ) -> Result<usize, StoreError>;

    async fn upsert_folder_chunks(
        &self,
        chunks: &[code_rag_types::FolderChunk],
        embeddings: Vec<Vec<f32>>,
    ) -> Result<usize, StoreError>;

    async fn upsert_file_chunks(
        &self,
        chunks: &[code_rag_types::FileChunk],
        embeddings: Vec<Vec<f32>>,
    ) -> Result<usize, StoreError>;

    // ---- deletes ----
    async fn delete_chunks_by_file(
        &self,
        table_name: &str,
        file_path: &str,
    ) -> Result<usize, StoreError>;

    async fn delete_chunks_by_project(
        &self,
        table_name: &str,
        project_name: &str,
    ) -> Result<usize, StoreError>;

    async fn delete_chunk_by_id(
        &self,
        table_name: &str,
        chunk_id: &str,
    ) -> Result<bool, StoreError>;

    async fn delete_chunks_by_ids(
        &self,
        table_name: &str,
        chunk_ids: &[String],
    ) -> Result<(), StoreError>;

    // ---- indexing ----
    async fn create_fts_indices(&self) -> Result<(), StoreError>;

    // ---- call-graph writes ----
    async fn upsert_call_edges(
        &self,
        edges: &[code_rag_types::CallEdge],
    ) -> Result<usize, StoreError>;

    async fn delete_edges_by_project(&self, project_name: &str) -> Result<(), StoreError>;
}

// ---------- LlmClient ----------

/// Stateless wrapper around an LLM provider (Gemini via rig-core today). The
/// chat target's `EngineError` `#[from]`-wraps [`LlmError`].
#[wagon]
#[async_trait]
pub trait LlmClient: Send + Sync {
    async fn generate(&self, prompt: &str) -> Result<String, LlmError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn llm_error_serde_roundtrip() {
        let err = LlmError::Generation("rate limited".into());
        let json = serde_json::to_string(&err).unwrap();
        let back: LlmError = serde_json::from_str(&json).unwrap();
        assert_eq!(format!("{back:?}"), format!("{err:?}"));
    }
}
