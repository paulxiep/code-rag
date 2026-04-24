use arrow_array::{
    Array, Float32Array, RecordBatch, RecordBatchIterator, StringArray, UInt64Array,
};
use futures::TryStreamExt;
use lancedb::{
    Connection, DistanceType, Table, connect,
    index::Index,
    index::scalar::{FtsIndexBuilder, FullTextSearchQuery},
    query::{ExecutableQuery, QueryBase},
};
use std::sync::Arc;
use thiserror::Error;

use code_rag_engine::text::build_searchable_text;
use code_rag_types::{
    CallEdge, CodeChunk, CrateChunk, FileChunk, FolderChunk, ModuleDocChunk, ReadmeChunk,
};

#[derive(Error, Debug)]
pub enum StoreError {
    #[error("database error: {0}")]
    Database(#[from] lancedb::Error),

    #[error("arrow error: {0}")]
    Arrow(#[from] arrow_schema::ArrowError),

    #[error("table '{0}' not found")]
    TableNotFound(String),

    #[error("schema mismatch: {0}")]
    SchemaMismatch(String),
}

const CODE_TABLE: &str = "code_chunks";
const README_TABLE: &str = "readme_chunks";
const CRATE_TABLE: &str = "crate_chunks";
const MODULE_DOC_TABLE: &str = "module_doc_chunks";
const CALL_EDGES_TABLE: &str = "call_edges";
/// A2: folder-level summary chunks.
pub const FOLDER_TABLE: &str = "folder_chunks";
/// A4: file-level summary chunks.
pub const FILE_TABLE: &str = "file_chunks";

/// Bump when the Arrow schema of any persisted table changes (column added,
/// removed, renamed, or retyped). The bump invalidates existing indexes —
/// `VectorStore::new()` refuses to open a DB whose `_schema_version` sidecar
/// disagrees with this constant. Recovery is `rm -rf <db_path>` followed by
/// a fresh `code-raptor ingest --full`.
///
/// Distinct from `code_rag_types::DERIVATION_VERSION`: derivation governs
/// chunk-content hashing (forces re-embed of unchanged source), schema
/// governs the on-disk column layout (forces a wipe).
pub const SCHEMA_VERSION: u32 = 1;
const SCHEMA_VERSION_FILE: &str = "_schema_version";

/// Read the on-disk schema version from `<db_path>/_schema_version`. Returns
/// `Ok(None)` if the sidecar doesn't exist (fresh DB or pre-sentinel DB);
/// `Ok(Some(N))` otherwise. Garbage in the file → `SchemaMismatch` so we
/// fail closed.
fn read_schema_version(db_path: &std::path::Path) -> Result<Option<u32>, StoreError> {
    let path = db_path.join(SCHEMA_VERSION_FILE);
    match std::fs::read_to_string(&path) {
        Ok(s) => s
            .trim()
            .parse::<u32>()
            .map(Some)
            .map_err(|e| StoreError::SchemaMismatch(format!(
                "{} is not a valid schema version: {}",
                path.display(),
                e
            ))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(StoreError::SchemaMismatch(format!(
            "could not read {}: {}",
            path.display(),
            e
        ))),
    }
}

/// Write the current `SCHEMA_VERSION` sidecar. Idempotent — no-op if the
/// file already exists with the matching value.
fn write_schema_version(db_path: &std::path::Path) -> Result<(), StoreError> {
    if matches!(read_schema_version(db_path)?, Some(v) if v == SCHEMA_VERSION) {
        return Ok(());
    }
    std::fs::create_dir_all(db_path).ok();
    let path = db_path.join(SCHEMA_VERSION_FILE);
    std::fs::write(&path, SCHEMA_VERSION.to_string()).map_err(|e| {
        StoreError::SchemaMismatch(format!("could not write {}: {}", path.display(), e))
    })
}

/// Refuse to open a DB whose on-disk schema disagrees with this binary.
/// Skips the check on a fresh / pre-sentinel DB (returns Ok); the first
/// successful table creation will write the sentinel.
///
/// On mismatch the error message tells the user how to recover (wipe + full
/// re-ingest). We deliberately don't auto-migrate — a schema bump means a
/// column changed, so existing rows are unreadable as-is.
fn check_schema_version_compat(db_path: &std::path::Path) -> Result<(), StoreError> {
    match read_schema_version(db_path)? {
        None => Ok(()), // fresh or legacy DB; first write stamps the sentinel
        Some(v) if v == SCHEMA_VERSION => Ok(()),
        Some(v) => Err(StoreError::SchemaMismatch(format!(
            "schema v{} on disk at {}, this binary expects v{}. Recover with: \
             rm -rf {} && code-raptor ingest <repo> --db-path {} --single-repo --full",
            v,
            db_path.display(),
            SCHEMA_VERSION,
            db_path.display(),
            db_path.display()
        ))),
    }
}

/// LanceDB-backed vector store for code and readme chunks.
pub struct VectorStore {
    conn: Connection,
    dimension: usize,
    db_path: std::path::PathBuf,
}

impl VectorStore {
    /// Connect to LanceDB at the given path (creates if not exists).
    ///
    /// If the directory already contains data, verifies the on-disk schema
    /// version matches `SCHEMA_VERSION` and refuses to open on mismatch.
    /// First write to a fresh DB will write the sentinel.
    pub async fn new(db_path: &str, embedding_dimension: usize) -> Result<Self, StoreError> {
        // Ensure parent directory exists (important for Docker bind mounts)
        if let Some(parent) = std::path::Path::new(db_path).parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let conn = connect(db_path).execute().await?;
        let db_path_buf = std::path::PathBuf::from(db_path);
        check_schema_version_compat(&db_path_buf)?;
        Ok(Self {
            conn,
            dimension: embedding_dimension,
            db_path: db_path_buf,
        })
    }

    pub fn dimension(&self) -> usize {
        self.dimension
    }

    // ========================================================================
    // Write operations (used by code-raptor)
    // ========================================================================

    /// Insert code chunks with their embeddings. Creates table if needed.
    ///
    /// `signature_embeddings` is aligned 1:1 with `chunks`; entries are `None`
    /// when the chunk has no signature (macros, statements, etc.). Passing an
    /// all-`None` vector is equivalent to pre-B5 single-vector ingest.
    pub async fn upsert_code_chunks(
        &self,
        chunks: &[CodeChunk],
        embeddings: Vec<Vec<f32>>,
        signature_embeddings: Vec<Option<Vec<f32>>>,
    ) -> Result<usize, StoreError> {
        if chunks.is_empty() {
            return Ok(0);
        }

        let batch = code_chunks_to_batch(chunks, embeddings, signature_embeddings, self.dimension)?;
        let count = batch.num_rows();

        self.upsert_batch(CODE_TABLE, "chunk_id", batch).await?;
        Ok(count)
    }

    /// Insert readme chunks with their embeddings. Creates table if needed.
    pub async fn upsert_readme_chunks(
        &self,
        chunks: &[ReadmeChunk],
        embeddings: Vec<Vec<f32>>,
    ) -> Result<usize, StoreError> {
        if chunks.is_empty() {
            return Ok(0);
        }

        let batch = readme_chunks_to_batch(chunks, embeddings, self.dimension)?;
        let count = batch.num_rows();

        self.upsert_batch(README_TABLE, "chunk_id", batch).await?;
        Ok(count)
    }

    /// Insert crate chunks with their embeddings. Creates table if needed.
    pub async fn upsert_crate_chunks(
        &self,
        chunks: &[CrateChunk],
        embeddings: Vec<Vec<f32>>,
    ) -> Result<usize, StoreError> {
        if chunks.is_empty() {
            return Ok(0);
        }

        let batch = crate_chunks_to_batch(chunks, embeddings, self.dimension)?;
        let count = batch.num_rows();

        self.upsert_batch(CRATE_TABLE, "chunk_id", batch).await?;
        Ok(count)
    }

    /// Insert module doc chunks with their embeddings. Creates table if needed.
    pub async fn upsert_module_doc_chunks(
        &self,
        chunks: &[ModuleDocChunk],
        embeddings: Vec<Vec<f32>>,
    ) -> Result<usize, StoreError> {
        if chunks.is_empty() {
            return Ok(0);
        }

        let batch = module_doc_chunks_to_batch(chunks, embeddings, self.dimension)?;
        let count = batch.num_rows();

        self.upsert_batch(MODULE_DOC_TABLE, "chunk_id", batch).await?;
        Ok(count)
    }

    /// A2: insert folder-summary chunks with their embeddings.
    /// Creates the `folder_chunks` table if needed.
    pub async fn upsert_folder_chunks(
        &self,
        chunks: &[FolderChunk],
        embeddings: Vec<Vec<f32>>,
    ) -> Result<usize, StoreError> {
        if chunks.is_empty() {
            return Ok(0);
        }

        let batch = folder_chunks_to_batch(chunks, embeddings, self.dimension)?;
        let count = batch.num_rows();

        self.upsert_batch(FOLDER_TABLE, "chunk_id", batch).await?;
        Ok(count)
    }

    /// A4: insert file-summary chunks with their embeddings.
    /// Creates the `file_chunks` table if needed.
    pub async fn upsert_file_chunks(
        &self,
        chunks: &[FileChunk],
        embeddings: Vec<Vec<f32>>,
    ) -> Result<usize, StoreError> {
        if chunks.is_empty() {
            return Ok(0);
        }

        let batch = file_chunks_to_batch(chunks, embeddings, self.dimension)?;
        let count = batch.num_rows();

        self.upsert_batch(FILE_TABLE, "chunk_id", batch).await?;
        Ok(count)
    }

    // ========================================================================
    // Read operations (used by code-rag-chat)
    // ========================================================================

    /// Search crate chunks by vector similarity. Returns (chunk, distance) pairs.
    pub async fn search_crates(
        &self,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<(CrateChunk, f32)>, StoreError> {
        let table = self.get_table(CRATE_TABLE).await?;

        let results = table
            .vector_search(query_embedding.to_vec())?
            .distance_type(DistanceType::L2)
            .limit(limit)
            .execute()
            .await?;

        batches_to_crate_chunks(results).await
    }

    /// Search module doc chunks by vector similarity. Returns (chunk, distance) pairs.
    pub async fn search_module_docs(
        &self,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<(ModuleDocChunk, f32)>, StoreError> {
        let table = self.get_table(MODULE_DOC_TABLE).await?;

        let results = table
            .vector_search(query_embedding.to_vec())?
            .distance_type(DistanceType::L2)
            .limit(limit)
            .execute()
            .await?;

        batches_to_module_doc_chunks(results).await
    }

    /// A2: search folder-summary chunks by vector similarity.
    pub async fn search_folders(
        &self,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<(FolderChunk, f32)>, StoreError> {
        let table = self.get_table(FOLDER_TABLE).await?;

        let results = table
            .vector_search(query_embedding.to_vec())?
            .distance_type(DistanceType::L2)
            .limit(limit)
            .execute()
            .await?;

        batches_to_folder_chunks(results).await
    }

    /// A4: search file-summary chunks by vector similarity.
    pub async fn search_files(
        &self,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<(FileChunk, f32)>, StoreError> {
        let table = self.get_table(FILE_TABLE).await?;

        let results = table
            .vector_search(query_embedding.to_vec())?
            .distance_type(DistanceType::L2)
            .limit(limit)
            .execute()
            .await?;

        batches_to_file_chunks(results).await
    }

    /// Search code chunks by vector similarity. Returns (chunk, distance) pairs.
    pub async fn search_code(
        &self,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<(CodeChunk, f32)>, StoreError> {
        let table = self.get_table(CODE_TABLE).await?;

        let results = table
            .vector_search(query_embedding.to_vec())?
            .column("vector")
            .distance_type(DistanceType::L2)
            .limit(limit)
            .execute()
            .await?;

        batches_to_code_chunks(results).await
    }

    /// B5: Search code chunks by signature-vector similarity. Rows whose
    /// `signature_vector` is null are skipped by LanceDB's vector search.
    /// Returns (chunk, distance) pairs — same shape as `search_code()`.
    pub async fn search_code_signatures(
        &self,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<(CodeChunk, f32)>, StoreError> {
        let table = self.get_table(CODE_TABLE).await?;

        let results = table
            .vector_search(query_embedding.to_vec())?
            .column("signature_vector")
            .distance_type(DistanceType::L2)
            .limit(limit)
            .execute()
            .await?;

        batches_to_code_chunks(results).await
    }

    /// Search readme chunks by vector similarity. Returns (chunk, distance) pairs.
    pub async fn search_readme(
        &self,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<(ReadmeChunk, f32)>, StoreError> {
        let table = self.get_table(README_TABLE).await?;

        let results = table
            .vector_search(query_embedding.to_vec())?
            .distance_type(DistanceType::L2)
            .limit(limit)
            .execute()
            .await?;

        batches_to_readme_chunks(results).await
    }

    /// Search all chunk types, return combined results with distances.
    pub async fn search_all(
        &self,
        query_embedding: &[f32],
        code_limit: usize,
        readme_limit: usize,
        crate_limit: usize,
        module_doc_limit: usize,
    ) -> Result<
        (
            Vec<(CodeChunk, f32)>,
            Vec<(ReadmeChunk, f32)>,
            Vec<(CrateChunk, f32)>,
            Vec<(ModuleDocChunk, f32)>,
        ),
        StoreError,
    > {
        let code = self.search_code(query_embedding, code_limit).await?;
        let readme = self.search_readme(query_embedding, readme_limit).await?;
        let crates = self
            .search_crates(query_embedding, crate_limit)
            .await
            .unwrap_or_default();
        let module_docs = self
            .search_module_docs(query_embedding, module_doc_limit)
            .await
            .unwrap_or_default();
        Ok((code, readme, crates, module_docs))
    }

    // ========================================================================
    // Hybrid search operations (B2: BM25 + semantic)
    // ========================================================================

    /// Code-appropriate FTS tokenizer config.
    /// `simple` tokenizer splits on non-alphanumeric (good for snake_case).
    /// Stemming and stop-word removal disabled (code identifiers aren't English words,
    /// and Rust keywords like `self`, `for`, `return` overlap with English stop words).
    fn code_fts_config() -> FtsIndexBuilder {
        FtsIndexBuilder::default()
            .base_tokenizer("simple".to_owned())
            .lower_case(true)
            .stem(false)
            .remove_stop_words(true)
    }

    /// Create FTS indices on all tables. Call after ingestion.
    pub async fn create_fts_indices(&self) -> Result<(), StoreError> {
        let tables_and_columns = [
            (CODE_TABLE, "searchable_text"),
            (README_TABLE, "content"),
            (CRATE_TABLE, "description"),
            (MODULE_DOC_TABLE, "doc_content"),
            // A2: FTS over the folder's rendered summary text.
            (FOLDER_TABLE, "summary_text"),
            // A4: FTS over the file's rendered summary text.
            (FILE_TABLE, "summary_text"),
        ];

        for (table_name, column) in &tables_and_columns {
            match self.conn.open_table(*table_name).execute().await {
                Ok(table) => {
                    table
                        .create_index(&[*column], Index::FTS(Self::code_fts_config()))
                        .replace(true)
                        .execute()
                        .await?;
                    tracing::info!("FTS index created on {}.{}", table_name, column);
                }
                Err(_) => {
                    tracing::debug!("Table {} not found, skipping FTS index", table_name);
                }
            }
        }

        Ok(())
    }

    /// Hybrid search code chunks: vector + FTS combined via RRF.
    /// Returns (chunk, relevance_score) pairs where score is higher=better.
    pub async fn hybrid_search_code(
        &self,
        query_text: &str,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<(CodeChunk, f32)>, StoreError> {
        let table = self.get_table(CODE_TABLE).await?;

        match table
            .vector_search(query_embedding.to_vec())?
            .column("vector")
            .distance_type(DistanceType::L2)
            .full_text_search(FullTextSearchQuery::new(query_text.to_string()))
            .limit(limit)
            .execute()
            .await
        {
            Ok(results) => batches_to_code_chunks_hybrid(results).await,
            Err(e) => {
                tracing::warn!(
                    "Hybrid search failed for {}, falling back to vector-only: {}",
                    CODE_TABLE,
                    e
                );
                let results = self.search_code(query_embedding, limit).await?;
                Ok(results
                    .into_iter()
                    .map(|(c, d)| (c, 1.0 / (1.0 + d)))
                    .collect())
            }
        }
    }

    /// Hybrid search readme chunks: vector + FTS combined via RRF.
    pub async fn hybrid_search_readme(
        &self,
        query_text: &str,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<(ReadmeChunk, f32)>, StoreError> {
        let table = self.get_table(README_TABLE).await?;

        match table
            .vector_search(query_embedding.to_vec())?
            .distance_type(DistanceType::L2)
            .full_text_search(FullTextSearchQuery::new(query_text.to_string()))
            .limit(limit)
            .execute()
            .await
        {
            Ok(results) => batches_to_readme_chunks_hybrid(results).await,
            Err(e) => {
                tracing::warn!(
                    "Hybrid search failed for {}, falling back to vector-only: {}",
                    README_TABLE,
                    e
                );
                let results = self.search_readme(query_embedding, limit).await?;
                Ok(results
                    .into_iter()
                    .map(|(c, d)| (c, 1.0 / (1.0 + d)))
                    .collect())
            }
        }
    }

    /// Hybrid search crate chunks: vector + FTS combined via RRF.
    pub async fn hybrid_search_crates(
        &self,
        query_text: &str,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<(CrateChunk, f32)>, StoreError> {
        let table = self.get_table(CRATE_TABLE).await?;

        match table
            .vector_search(query_embedding.to_vec())?
            .distance_type(DistanceType::L2)
            .full_text_search(FullTextSearchQuery::new(query_text.to_string()))
            .limit(limit)
            .execute()
            .await
        {
            Ok(results) => batches_to_crate_chunks_hybrid(results).await,
            Err(e) => {
                tracing::warn!(
                    "Hybrid search failed for {}, falling back to vector-only: {}",
                    CRATE_TABLE,
                    e
                );
                let results = self.search_crates(query_embedding, limit).await?;
                Ok(results
                    .into_iter()
                    .map(|(c, d)| (c, 1.0 / (1.0 + d)))
                    .collect())
            }
        }
    }

    /// A2: hybrid search folder-summary chunks (vector + FTS, LanceDB RRF).
    /// Returns (chunk, relevance_score) tuples where score is higher=better.
    /// Falls back to vector-only if FTS index is missing.
    pub async fn hybrid_search_folders(
        &self,
        query_text: &str,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<(FolderChunk, f32)>, StoreError> {
        let table = self.get_table(FOLDER_TABLE).await?;

        match table
            .vector_search(query_embedding.to_vec())?
            .distance_type(DistanceType::L2)
            .full_text_search(FullTextSearchQuery::new(query_text.to_string()))
            .limit(limit)
            .execute()
            .await
        {
            Ok(results) => batches_to_folder_chunks_hybrid(results).await,
            Err(e) => {
                tracing::warn!(
                    "Hybrid search failed for {}, falling back to vector-only: {}",
                    FOLDER_TABLE,
                    e
                );
                let results = self.search_folders(query_embedding, limit).await?;
                Ok(results
                    .into_iter()
                    .map(|(c, d)| (c, 1.0 / (1.0 + d)))
                    .collect())
            }
        }
    }

    /// A4: hybrid search file-summary chunks (vector + FTS, LanceDB RRF).
    /// Returns (chunk, relevance_score) tuples where score is higher=better.
    /// Falls back to vector-only if FTS index is missing.
    pub async fn hybrid_search_files(
        &self,
        query_text: &str,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<(FileChunk, f32)>, StoreError> {
        let table = self.get_table(FILE_TABLE).await?;

        match table
            .vector_search(query_embedding.to_vec())?
            .distance_type(DistanceType::L2)
            .full_text_search(FullTextSearchQuery::new(query_text.to_string()))
            .limit(limit)
            .execute()
            .await
        {
            Ok(results) => batches_to_file_chunks_hybrid(results).await,
            Err(e) => {
                tracing::warn!(
                    "Hybrid search failed for {}, falling back to vector-only: {}",
                    FILE_TABLE,
                    e
                );
                let results = self.search_files(query_embedding, limit).await?;
                Ok(results
                    .into_iter()
                    .map(|(c, d)| (c, 1.0 / (1.0 + d)))
                    .collect())
            }
        }
    }

    /// Hybrid search module doc chunks: vector + FTS combined via RRF.
    pub async fn hybrid_search_module_docs(
        &self,
        query_text: &str,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<(ModuleDocChunk, f32)>, StoreError> {
        let table = self.get_table(MODULE_DOC_TABLE).await?;

        match table
            .vector_search(query_embedding.to_vec())?
            .distance_type(DistanceType::L2)
            .full_text_search(FullTextSearchQuery::new(query_text.to_string()))
            .limit(limit)
            .execute()
            .await
        {
            Ok(results) => batches_to_module_doc_chunks_hybrid(results).await,
            Err(e) => {
                tracing::warn!(
                    "Hybrid search failed for {}, falling back to vector-only: {}",
                    MODULE_DOC_TABLE,
                    e
                );
                let results = self.search_module_docs(query_embedding, limit).await?;
                Ok(results
                    .into_iter()
                    .map(|(c, d)| (c, 1.0 / (1.0 + d)))
                    .collect())
            }
        }
    }

    /// Hybrid search all chunk types in parallel. Returns (chunk, relevance_score) tuples.
    pub async fn hybrid_search_all(
        &self,
        query_text: &str,
        query_embedding: &[f32],
        code_limit: usize,
        readme_limit: usize,
        crate_limit: usize,
        module_doc_limit: usize,
    ) -> Result<
        (
            Vec<(CodeChunk, f32)>,
            Vec<(ReadmeChunk, f32)>,
            Vec<(CrateChunk, f32)>,
            Vec<(ModuleDocChunk, f32)>,
        ),
        StoreError,
    > {
        let (code, readme, crates, module_docs) = tokio::join!(
            self.hybrid_search_code(query_text, query_embedding, code_limit),
            self.hybrid_search_readme(query_text, query_embedding, readme_limit),
            self.hybrid_search_crates(query_text, query_embedding, crate_limit),
            self.hybrid_search_module_docs(query_text, query_embedding, module_doc_limit),
        );
        Ok((
            code?,
            readme?,
            crates.unwrap_or_default(),
            module_docs.unwrap_or_default(),
        ))
    }

    // ========================================================================
    // Delete operations (V1.1: for incremental ingestion)
    // ========================================================================

    /// Delete all chunks from a file
    pub async fn delete_chunks_by_file(
        &self,
        table_name: &str,
        file_path: &str,
    ) -> Result<usize, StoreError> {
        let table = match self.conn.open_table(table_name).execute().await {
            Ok(t) => t,
            Err(_) => return Ok(0), // Table doesn't exist, nothing to delete
        };

        let predicate = format!("file_path = '{}'", file_path.replace("'", "''"));
        table.delete(&predicate).await?;

        // LanceDB delete doesn't return count, so we return 0 as placeholder
        // The actual deletion happened if no error
        Ok(0)
    }

    /// Delete all chunks from a project
    pub async fn delete_chunks_by_project(
        &self,
        table_name: &str,
        project_name: &str,
    ) -> Result<usize, StoreError> {
        let table = match self.conn.open_table(table_name).execute().await {
            Ok(t) => t,
            Err(_) => return Ok(0),
        };

        let predicate = format!("project_name = '{}'", project_name.replace("'", "''"));
        table.delete(&predicate).await?;

        Ok(0)
    }

    /// Delete chunk by UUID
    pub async fn delete_chunk_by_id(
        &self,
        table_name: &str,
        chunk_id: &str,
    ) -> Result<bool, StoreError> {
        let table = match self.conn.open_table(table_name).execute().await {
            Ok(t) => t,
            Err(_) => return Ok(false),
        };

        let predicate = format!("chunk_id = '{}'", chunk_id);
        table.delete(&predicate).await?;

        Ok(true)
    }

    /// Query chunks by file path (for incremental comparison)
    /// Returns (chunk_id, content_hash) pairs
    pub async fn get_chunks_by_file(
        &self,
        table_name: &str,
        file_path: &str,
    ) -> Result<Vec<(String, String)>, StoreError> {
        let table = match self.conn.open_table(table_name).execute().await {
            Ok(t) => t,
            Err(_) => return Ok(Vec::new()),
        };

        let results: Vec<RecordBatch> = table
            .query()
            .only_if(format!("file_path = '{}'", file_path.replace("'", "''")))
            .select(lancedb::query::Select::columns(&[
                "chunk_id",
                "content_hash",
            ]))
            .execute()
            .await?
            .try_collect()
            .await?;

        let mut pairs = Vec::new();
        for batch in results {
            let chunk_ids = batch
                .column_by_name("chunk_id")
                .and_then(|c| c.as_any().downcast_ref::<StringArray>());
            let content_hashes = batch
                .column_by_name("content_hash")
                .and_then(|c| c.as_any().downcast_ref::<StringArray>());

            if let (Some(ids), Some(hashes)) = (chunk_ids, content_hashes) {
                for i in 0..batch.num_rows() {
                    pairs.push((ids.value(i).to_string(), hashes.value(i).to_string()));
                }
            }
        }

        Ok(pairs)
    }

    // ========================================================================
    // V1.3: Incremental ingestion support
    // ========================================================================

    /// Get file-level index for a project: maps file_path → (content_hash, chunk_ids).
    /// For crate_chunks, use `file_path_column = "crate_name"`.
    pub async fn get_file_index(
        &self,
        table_name: &str,
        project_name: &str,
        file_path_column: &str,
    ) -> Result<std::collections::HashMap<String, (String, Vec<String>)>, StoreError> {
        use std::collections::HashMap;

        let table = match self.conn.open_table(table_name).execute().await {
            Ok(t) => t,
            Err(_) => return Ok(HashMap::new()),
        };

        let results: Vec<RecordBatch> = table
            .query()
            .only_if(format!(
                "project_name = '{}'",
                project_name.replace("'", "''")
            ))
            .select(lancedb::query::Select::columns(&[
                file_path_column,
                "chunk_id",
                "content_hash",
            ]))
            .execute()
            .await?
            .try_collect()
            .await?;

        let mut index: HashMap<String, (String, Vec<String>)> = HashMap::new();

        for batch in results {
            let paths = batch
                .column_by_name(file_path_column)
                .and_then(|c| c.as_any().downcast_ref::<StringArray>());
            let chunk_ids = batch
                .column_by_name("chunk_id")
                .and_then(|c| c.as_any().downcast_ref::<StringArray>());
            let hashes = batch
                .column_by_name("content_hash")
                .and_then(|c| c.as_any().downcast_ref::<StringArray>());

            if let (Some(paths), Some(ids), Some(hashes)) = (paths, chunk_ids, hashes) {
                for i in 0..batch.num_rows() {
                    let path = paths.value(i).to_string();
                    let chunk_id = ids.value(i).to_string();
                    let hash = hashes.value(i).to_string();

                    index
                        .entry(path)
                        .and_modify(|(_, ids)| ids.push(chunk_id.clone()))
                        .or_insert_with(|| (hash, vec![chunk_id]));
                }
            }
        }

        Ok(index)
    }

    /// Batch delete chunks by their UUIDs (batched in groups of 100).
    pub async fn delete_chunks_by_ids(
        &self,
        table_name: &str,
        chunk_ids: &[String],
    ) -> Result<(), StoreError> {
        if chunk_ids.is_empty() {
            return Ok(());
        }

        let table = self.conn.open_table(table_name).execute().await?;

        for batch in chunk_ids.chunks(100) {
            let ids_str: String = batch
                .iter()
                .map(|id| format!("'{}'", id.replace("'", "''")))
                .collect::<Vec<_>>()
                .join(", ");

            table.delete(&format!("chunk_id IN ({})", ids_str)).await?;
        }

        Ok(())
    }

    /// Check embedding model version for a project. Returns None if no chunks exist.
    pub async fn get_embedding_model_version(
        &self,
        project_name: &str,
    ) -> Result<Option<String>, StoreError> {
        let table = match self.conn.open_table(CODE_TABLE).execute().await {
            Ok(t) => t,
            Err(_) => return Ok(None),
        };

        let results: Vec<RecordBatch> = table
            .query()
            .only_if(format!(
                "project_name = '{}'",
                project_name.replace("'", "''")
            ))
            .select(lancedb::query::Select::columns(&[
                "embedding_model_version",
            ]))
            .limit(1)
            .execute()
            .await?
            .try_collect()
            .await?;

        for batch in results {
            if let Some(versions) = batch
                .column_by_name("embedding_model_version")
                .and_then(|c| c.as_any().downcast_ref::<StringArray>())
                && batch.num_rows() > 0
            {
                return Ok(Some(versions.value(0).to_string()));
            }
        }

        Ok(None)
    }

    pub async fn list_projects(&self) -> Result<Vec<String>, StoreError> {
        let table = match self.conn.open_table(CODE_TABLE).execute().await {
            Ok(t) => t,
            Err(_) => return Ok(Vec::new()), // No data yet
        };

        // Query all project names
        let batches: Vec<RecordBatch> = table
            .query()
            .select(lancedb::query::Select::columns(&["project_name"]))
            .execute()
            .await?
            .try_collect()
            .await?;

        // Extract unique project names (non-nullable since V1.3)
        let mut projects: Vec<String> = batches
            .iter()
            .flat_map(|batch| {
                batch
                    .column_by_name("project_name")
                    .and_then(|c| c.as_any().downcast_ref::<StringArray>())
                    .map(|arr| {
                        (0..arr.len())
                            .map(|i| arr.value(i).to_string())
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default()
            })
            .collect();

        // Deduplicate and sort
        projects.sort();
        projects.dedup();

        Ok(projects)
    }

    // ========================================================================
    // Internal helpers
    // ========================================================================

    /// Upsert a record batch into `table_name`, merging on `key_column`.
    /// On an existing table, rows whose `key_column` value matches an
    /// existing row are updated in place; new keys are inserted. This is
    /// the actual upsert semantics the public `upsert_*` API names imply
    /// — the previous implementation called plain `table.add()`, which
    /// silently appended duplicate rows whenever a chunk_id collided
    /// (very common because chunk_id is content-deterministic).
    async fn upsert_batch(
        &self,
        table_name: &str,
        key_column: &str,
        batch: RecordBatch,
    ) -> Result<(), StoreError> {
        let schema = batch.schema();

        // Try to open existing table, create if not exists
        match self.conn.open_table(table_name).execute().await {
            Ok(table) => {
                let batches = RecordBatchIterator::new(vec![Ok(batch)], schema);
                let mut merge = table.merge_insert(&[key_column]);
                merge
                    .when_matched_update_all(None)
                    .when_not_matched_insert_all();
                merge.execute(Box::new(batches)).await?;
            }
            Err(_) => {
                let batches = RecordBatchIterator::new(vec![Ok(batch)], schema);
                self.conn
                    .create_table(table_name, batches)
                    .execute()
                    .await?;
                // Stamp the schema version on first successful table
                // creation. Idempotent — `write_schema_version` is a no-op
                // if the sidecar already exists with the same value.
                write_schema_version(&self.db_path)?;
            }
        }

        Ok(())
    }

    async fn get_table(&self, name: &str) -> Result<Table, StoreError> {
        self.conn
            .open_table(name)
            .execute()
            .await
            .map_err(|_| StoreError::TableNotFound(name.to_string()))
    }

    // ========================================================================
    // Call edges (C1: Graph RAG)
    // ========================================================================

    /// Insert call graph edges. Creates the call_edges table if needed.
    /// No vector column — pure scalar table.
    pub async fn upsert_call_edges(&self, edges: &[CallEdge]) -> Result<usize, StoreError> {
        if edges.is_empty() {
            return Ok(0);
        }

        let batch = call_edges_to_batch(edges)?;
        let count = batch.num_rows();
        self.upsert_batch(CALL_EDGES_TABLE, "edge_id", batch).await?;
        Ok(count)
    }

    /// Get all edges where callee_chunk_id matches (i.e., callers of the given chunk).
    pub async fn get_callers(
        &self,
        callee_chunk_id: &str,
        project: Option<&str>,
    ) -> Result<Vec<CallEdge>, StoreError> {
        let mut filter = format!("callee_chunk_id = '{}'", callee_chunk_id.replace("'", "''"));
        if let Some(p) = project {
            filter.push_str(&format!(" AND project_name = '{}'", p.replace("'", "''")));
        }
        self.query_call_edges(&filter).await
    }

    /// Get all edges where caller_chunk_id matches (i.e., callees of the given chunk).
    pub async fn get_callees(
        &self,
        caller_chunk_id: &str,
        project: Option<&str>,
    ) -> Result<Vec<CallEdge>, StoreError> {
        let mut filter = format!("caller_chunk_id = '{}'", caller_chunk_id.replace("'", "''"));
        if let Some(p) = project {
            filter.push_str(&format!(" AND project_name = '{}'", p.replace("'", "''")));
        }
        self.query_call_edges(&filter).await
    }

    /// Get all edges for a project (for building a full CallGraph).
    pub async fn get_all_edges(&self, project_name: &str) -> Result<Vec<CallEdge>, StoreError> {
        let filter = format!("project_name = '{}'", project_name.replace("'", "''"));
        self.query_call_edges(&filter).await
    }

    /// Delete all edges for a project (before re-resolving).
    pub async fn delete_edges_by_project(&self, project_name: &str) -> Result<(), StoreError> {
        let table = match self.conn.open_table(CALL_EDGES_TABLE).execute().await {
            Ok(t) => t,
            Err(_) => return Ok(()), // Table doesn't exist, nothing to delete
        };

        let predicate = format!("project_name = '{}'", project_name.replace("'", "''"));
        table.delete(&predicate).await?;
        Ok(())
    }

    /// Helper: query call_edges table with a filter predicate.
    async fn query_call_edges(&self, filter: &str) -> Result<Vec<CallEdge>, StoreError> {
        let table = match self.conn.open_table(CALL_EDGES_TABLE).execute().await {
            Ok(t) => t,
            Err(_) => return Ok(Vec::new()), // Table doesn't exist yet
        };

        let results: Vec<RecordBatch> = table
            .query()
            .only_if(filter)
            .execute()
            .await?
            .try_collect()
            .await?;

        let mut edges = Vec::new();
        for batch in &results {
            edges.extend(extract_call_edges_from_batch(batch)?);
        }
        Ok(edges)
    }

    /// Fetch full CodeChunks by their chunk IDs (for graph-resolved lookups).
    pub async fn get_chunks_by_ids(
        &self,
        chunk_ids: &[String],
    ) -> Result<Vec<CodeChunk>, StoreError> {
        if chunk_ids.is_empty() {
            return Ok(Vec::new());
        }

        let table = self.get_table(CODE_TABLE).await?;
        let mut all_chunks = Vec::new();

        // Batch queries to avoid overly-long SQL predicates
        for batch_ids in chunk_ids.chunks(100) {
            let ids_str: String = batch_ids
                .iter()
                .map(|id| format!("'{}'", id.replace("'", "''")))
                .collect::<Vec<_>>()
                .join(", ");

            let results: Vec<RecordBatch> = table
                .query()
                .only_if(format!("chunk_id IN ({})", ids_str))
                .execute()
                .await?
                .try_collect()
                .await?;

            for batch in &results {
                let chunks = extract_code_chunks_from_batch_with_score(batch, "_distance");
                // If _distance column doesn't exist (no vector search), try without score
                let chunks = match chunks {
                    Ok(c) => c.into_iter().map(|(chunk, _)| chunk).collect(),
                    Err(_) => extract_code_chunks_no_score(batch)?,
                };
                all_chunks.extend(chunks);
            }
        }
        Ok(all_chunks)
    }
}

// ============================================================================
// Arrow conversion functions (pure, no side effects)
// ============================================================================

fn call_edges_to_batch(edges: &[CallEdge]) -> Result<RecordBatch, StoreError> {
    use arrow_array::UInt8Array;

    let edge_ids: StringArray = edges.iter().map(|e| Some(e.edge_id.as_str())).collect();
    let caller_chunk_ids: StringArray = edges
        .iter()
        .map(|e| Some(e.caller_chunk_id.as_str()))
        .collect();
    let callee_chunk_ids: StringArray = edges
        .iter()
        .map(|e| Some(e.callee_chunk_id.as_str()))
        .collect();
    let caller_identifiers: StringArray = edges
        .iter()
        .map(|e| Some(e.caller_identifier.as_str()))
        .collect();
    let callee_identifiers: StringArray = edges
        .iter()
        .map(|e| Some(e.callee_identifier.as_str()))
        .collect();
    let caller_files: StringArray = edges.iter().map(|e| Some(e.caller_file.as_str())).collect();
    let callee_files: StringArray = edges.iter().map(|e| Some(e.callee_file.as_str())).collect();
    let project_names: StringArray = edges
        .iter()
        .map(|e| Some(e.project_name.as_str()))
        .collect();
    let resolution_tiers: UInt8Array = edges.iter().map(|e| Some(e.resolution_tier)).collect();

    let schema = Arc::new(arrow_schema::Schema::new(vec![
        arrow_schema::Field::new("edge_id", arrow_schema::DataType::Utf8, false),
        arrow_schema::Field::new("caller_chunk_id", arrow_schema::DataType::Utf8, false),
        arrow_schema::Field::new("callee_chunk_id", arrow_schema::DataType::Utf8, false),
        arrow_schema::Field::new("caller_identifier", arrow_schema::DataType::Utf8, false),
        arrow_schema::Field::new("callee_identifier", arrow_schema::DataType::Utf8, false),
        arrow_schema::Field::new("caller_file", arrow_schema::DataType::Utf8, false),
        arrow_schema::Field::new("callee_file", arrow_schema::DataType::Utf8, false),
        arrow_schema::Field::new("project_name", arrow_schema::DataType::Utf8, false),
        arrow_schema::Field::new("resolution_tier", arrow_schema::DataType::UInt8, false),
    ]));

    Ok(RecordBatch::try_new(
        schema,
        vec![
            Arc::new(edge_ids),
            Arc::new(caller_chunk_ids),
            Arc::new(callee_chunk_ids),
            Arc::new(caller_identifiers),
            Arc::new(callee_identifiers),
            Arc::new(caller_files),
            Arc::new(callee_files),
            Arc::new(project_names),
            Arc::new(resolution_tiers),
        ],
    )?)
}

fn extract_call_edges_from_batch(batch: &RecordBatch) -> Result<Vec<CallEdge>, StoreError> {
    use arrow_array::UInt8Array;

    let col = |name: &str| -> Result<&StringArray, StoreError> {
        batch
            .column_by_name(name)
            .and_then(|c| c.as_any().downcast_ref::<StringArray>())
            .ok_or_else(|| StoreError::SchemaMismatch(name.into()))
    };

    let edge_ids = col("edge_id")?;
    let caller_chunk_ids = col("caller_chunk_id")?;
    let callee_chunk_ids = col("callee_chunk_id")?;
    let caller_identifiers = col("caller_identifier")?;
    let callee_identifiers = col("callee_identifier")?;
    let caller_files = col("caller_file")?;
    let callee_files = col("callee_file")?;
    let project_names = col("project_name")?;
    let resolution_tiers = batch
        .column_by_name("resolution_tier")
        .and_then(|c| c.as_any().downcast_ref::<UInt8Array>())
        .ok_or_else(|| StoreError::SchemaMismatch("resolution_tier".into()))?;

    let edges = (0..batch.num_rows())
        .map(|i| CallEdge {
            edge_id: edge_ids.value(i).to_string(),
            caller_chunk_id: caller_chunk_ids.value(i).to_string(),
            callee_chunk_id: callee_chunk_ids.value(i).to_string(),
            caller_identifier: caller_identifiers.value(i).to_string(),
            callee_identifier: callee_identifiers.value(i).to_string(),
            caller_file: caller_files.value(i).to_string(),
            callee_file: callee_files.value(i).to_string(),
            project_name: project_names.value(i).to_string(),
            resolution_tier: resolution_tiers.value(i),
        })
        .collect();

    Ok(edges)
}

/// Extract CodeChunks from a RecordBatch without requiring a distance/score column.
/// Used by `get_chunks_by_ids` where we're doing a scalar filter query, not vector search.
fn extract_code_chunks_no_score(batch: &RecordBatch) -> Result<Vec<CodeChunk>, StoreError> {
    let col = |name: &str| -> Result<&StringArray, StoreError> {
        batch
            .column_by_name(name)
            .and_then(|c| c.as_any().downcast_ref::<StringArray>())
            .ok_or_else(|| StoreError::SchemaMismatch(name.into()))
    };

    let file_paths = col("file_path")?;
    let languages = col("language")?;
    let identifiers = col("identifier")?;
    let node_types = col("node_type")?;
    let code_contents = col("code_content")?;
    let chunk_ids = col("chunk_id")?;
    let content_hashes = col("content_hash")?;
    let model_versions = col("embedding_model_version")?;
    let project_names = col("project_name")?;

    let start_lines = batch
        .column_by_name("start_line")
        .and_then(|c| c.as_any().downcast_ref::<UInt64Array>())
        .ok_or_else(|| StoreError::SchemaMismatch("start_line".into()))?;

    let docstrings = batch
        .column_by_name("docstring")
        .and_then(|c| c.as_any().downcast_ref::<StringArray>());
    let signatures = batch
        .column_by_name("signature")
        .and_then(|c| c.as_any().downcast_ref::<StringArray>());

    let nullable_string = |arr: Option<&StringArray>, i: usize| -> Option<String> {
        arr.filter(|a| !a.is_null(i))
            .map(|a| a.value(i).to_string())
    };

    let chunks = (0..batch.num_rows())
        .map(|i| CodeChunk {
            file_path: file_paths.value(i).to_string(),
            language: languages.value(i).to_string(),
            identifier: identifiers.value(i).to_string(),
            node_type: node_types.value(i).to_string(),
            code_content: code_contents.value(i).to_string(),
            start_line: start_lines.value(i) as usize,
            project_name: project_names.value(i).to_string(),
            docstring: nullable_string(docstrings, i),
            signature: nullable_string(signatures, i),
            chunk_id: chunk_ids.value(i).to_string(),
            content_hash: content_hashes.value(i).to_string(),
            embedding_model_version: model_versions.value(i).to_string(),
        })
        .collect();

    Ok(chunks)
}

fn code_chunks_to_batch(
    chunks: &[CodeChunk],
    embeddings: Vec<Vec<f32>>,
    signature_embeddings: Vec<Option<Vec<f32>>>,
    dim: usize,
) -> Result<RecordBatch, StoreError> {
    use arrow_array::builder::FixedSizeListBuilder;
    use std::sync::Arc;

    let file_paths: StringArray = chunks.iter().map(|c| Some(c.file_path.as_str())).collect();
    let languages: StringArray = chunks.iter().map(|c| Some(c.language.as_str())).collect();
    let identifiers: StringArray = chunks.iter().map(|c| Some(c.identifier.as_str())).collect();
    let node_types: StringArray = chunks.iter().map(|c| Some(c.node_type.as_str())).collect();
    let code_contents: StringArray = chunks
        .iter()
        .map(|c| Some(c.code_content.as_str()))
        .collect();
    let start_lines: UInt64Array = chunks.iter().map(|c| Some(c.start_line as u64)).collect();
    let project_names: StringArray = chunks
        .iter()
        .map(|c| Some(c.project_name.as_str()))
        .collect();
    let docstrings: StringArray = chunks.iter().map(|c| c.docstring.as_deref()).collect();

    // B3: signature and searchable_text columns
    let signatures: StringArray = chunks.iter().map(|c| c.signature.as_deref()).collect();
    let searchable_texts: StringArray = chunks
        .iter()
        .map(|c| {
            Some(build_searchable_text(
                &c.identifier,
                c.signature.as_deref(),
                c.docstring.as_deref(),
            ))
        })
        .collect();

    // New V1.1 fields
    let chunk_ids: StringArray = chunks.iter().map(|c| Some(c.chunk_id.as_str())).collect();
    let content_hashes: StringArray = chunks
        .iter()
        .map(|c| Some(c.content_hash.as_str()))
        .collect();
    let model_versions: StringArray = chunks
        .iter()
        .map(|c| Some(c.embedding_model_version.as_str()))
        .collect();

    // Build fixed-size vector column
    let mut vector_builder =
        FixedSizeListBuilder::new(arrow_array::builder::Float32Builder::new(), dim as i32);

    for emb in &embeddings {
        vector_builder.values().append_slice(emb);
        vector_builder.append(true);
    }

    let vectors = vector_builder.finish();

    // B5: signature_vector column (nullable). Rows without a signature embedding
    // append null; the inner float builder still needs `dim` zero values so the
    // fixed-size list stays consistent.
    let mut sig_vector_builder =
        FixedSizeListBuilder::new(arrow_array::builder::Float32Builder::new(), dim as i32);
    for sig_emb in &signature_embeddings {
        match sig_emb {
            Some(emb) => {
                sig_vector_builder.values().append_slice(emb);
                sig_vector_builder.append(true);
            }
            None => {
                sig_vector_builder
                    .values()
                    .append_slice(&vec![0.0_f32; dim]);
                sig_vector_builder.append(false);
            }
        }
    }
    let signature_vectors = sig_vector_builder.finish();

    let schema = Arc::new(arrow_schema::Schema::new(vec![
        arrow_schema::Field::new("file_path", arrow_schema::DataType::Utf8, false),
        arrow_schema::Field::new("language", arrow_schema::DataType::Utf8, false),
        arrow_schema::Field::new("identifier", arrow_schema::DataType::Utf8, false),
        arrow_schema::Field::new("node_type", arrow_schema::DataType::Utf8, false),
        arrow_schema::Field::new("code_content", arrow_schema::DataType::Utf8, false),
        arrow_schema::Field::new("start_line", arrow_schema::DataType::UInt64, false),
        arrow_schema::Field::new("project_name", arrow_schema::DataType::Utf8, false),
        arrow_schema::Field::new("docstring", arrow_schema::DataType::Utf8, true),
        arrow_schema::Field::new("signature", arrow_schema::DataType::Utf8, true),
        arrow_schema::Field::new("searchable_text", arrow_schema::DataType::Utf8, false),
        arrow_schema::Field::new("chunk_id", arrow_schema::DataType::Utf8, false),
        arrow_schema::Field::new("content_hash", arrow_schema::DataType::Utf8, false),
        arrow_schema::Field::new(
            "embedding_model_version",
            arrow_schema::DataType::Utf8,
            false,
        ),
        arrow_schema::Field::new(
            "vector",
            arrow_schema::DataType::FixedSizeList(
                Arc::new(arrow_schema::Field::new(
                    "item",
                    arrow_schema::DataType::Float32,
                    true,
                )),
                dim as i32,
            ),
            false,
        ),
        arrow_schema::Field::new(
            "signature_vector",
            arrow_schema::DataType::FixedSizeList(
                Arc::new(arrow_schema::Field::new(
                    "item",
                    arrow_schema::DataType::Float32,
                    true,
                )),
                dim as i32,
            ),
            true,
        ),
    ]));

    Ok(RecordBatch::try_new(
        schema,
        vec![
            Arc::new(file_paths),
            Arc::new(languages),
            Arc::new(identifiers),
            Arc::new(node_types),
            Arc::new(code_contents),
            Arc::new(start_lines),
            Arc::new(project_names),
            Arc::new(docstrings),
            Arc::new(signatures),
            Arc::new(searchable_texts),
            Arc::new(chunk_ids),
            Arc::new(content_hashes),
            Arc::new(model_versions),
            Arc::new(vectors),
            Arc::new(signature_vectors),
        ],
    )?)
}

fn readme_chunks_to_batch(
    chunks: &[ReadmeChunk],
    embeddings: Vec<Vec<f32>>,
    dim: usize,
) -> Result<RecordBatch, StoreError> {
    use arrow_array::builder::FixedSizeListBuilder;

    let file_paths: StringArray = chunks.iter().map(|c| Some(c.file_path.as_str())).collect();
    let project_names: StringArray = chunks
        .iter()
        .map(|c| Some(c.project_name.as_str()))
        .collect();
    let contents: StringArray = chunks.iter().map(|c| Some(c.content.as_str())).collect();

    // New V1.1 fields
    let chunk_ids: StringArray = chunks.iter().map(|c| Some(c.chunk_id.as_str())).collect();
    let content_hashes: StringArray = chunks
        .iter()
        .map(|c| Some(c.content_hash.as_str()))
        .collect();
    let model_versions: StringArray = chunks
        .iter()
        .map(|c| Some(c.embedding_model_version.as_str()))
        .collect();

    let mut vector_builder =
        FixedSizeListBuilder::new(arrow_array::builder::Float32Builder::new(), dim as i32);

    for emb in &embeddings {
        vector_builder.values().append_slice(emb);
        vector_builder.append(true);
    }

    let vectors = vector_builder.finish();

    let schema = Arc::new(arrow_schema::Schema::new(vec![
        arrow_schema::Field::new("file_path", arrow_schema::DataType::Utf8, false),
        arrow_schema::Field::new("project_name", arrow_schema::DataType::Utf8, false),
        arrow_schema::Field::new("content", arrow_schema::DataType::Utf8, false),
        arrow_schema::Field::new("chunk_id", arrow_schema::DataType::Utf8, false),
        arrow_schema::Field::new("content_hash", arrow_schema::DataType::Utf8, false),
        arrow_schema::Field::new(
            "embedding_model_version",
            arrow_schema::DataType::Utf8,
            false,
        ),
        arrow_schema::Field::new(
            "vector",
            arrow_schema::DataType::FixedSizeList(
                Arc::new(arrow_schema::Field::new(
                    "item",
                    arrow_schema::DataType::Float32,
                    true,
                )),
                dim as i32,
            ),
            false,
        ),
    ]));

    Ok(RecordBatch::try_new(
        schema,
        vec![
            Arc::new(file_paths),
            Arc::new(project_names),
            Arc::new(contents),
            Arc::new(chunk_ids),
            Arc::new(content_hashes),
            Arc::new(model_versions),
            Arc::new(vectors),
        ],
    )?)
}

fn crate_chunks_to_batch(
    chunks: &[CrateChunk],
    embeddings: Vec<Vec<f32>>,
    dim: usize,
) -> Result<RecordBatch, StoreError> {
    use arrow_array::builder::FixedSizeListBuilder;
    use arrow_array::{ArrayRef, ListArray};
    use arrow_buffer::OffsetBuffer;

    let crate_names: StringArray = chunks.iter().map(|c| Some(c.crate_name.as_str())).collect();
    let crate_paths: StringArray = chunks.iter().map(|c| Some(c.crate_path.as_str())).collect();
    let descriptions: StringArray = chunks.iter().map(|c| c.description.as_deref()).collect();
    let project_names: StringArray = chunks
        .iter()
        .map(|c| Some(c.project_name.as_str()))
        .collect();

    // Build list array for dependencies (V1.1: List<Utf8> instead of CSV string)
    let mut offsets = vec![0i32];
    let mut values: Vec<Option<&str>> = vec![];

    for chunk in chunks {
        for dep in &chunk.dependencies {
            values.push(Some(dep.as_str()));
        }
        offsets.push(values.len() as i32);
    }

    let values_array: StringArray = values.into_iter().collect();
    let dependencies = ListArray::new(
        Arc::new(arrow_schema::Field::new(
            "item",
            arrow_schema::DataType::Utf8,
            true,
        )),
        OffsetBuffer::new(offsets.into()),
        Arc::new(values_array),
        None,
    );

    // New V1.1 fields
    let chunk_ids: StringArray = chunks.iter().map(|c| Some(c.chunk_id.as_str())).collect();
    let content_hashes: StringArray = chunks
        .iter()
        .map(|c| Some(c.content_hash.as_str()))
        .collect();
    let model_versions: StringArray = chunks
        .iter()
        .map(|c| Some(c.embedding_model_version.as_str()))
        .collect();

    let mut vector_builder =
        FixedSizeListBuilder::new(arrow_array::builder::Float32Builder::new(), dim as i32);

    for emb in &embeddings {
        vector_builder.values().append_slice(emb);
        vector_builder.append(true);
    }

    let vectors = vector_builder.finish();

    let schema = Arc::new(arrow_schema::Schema::new(vec![
        arrow_schema::Field::new("crate_name", arrow_schema::DataType::Utf8, false),
        arrow_schema::Field::new("crate_path", arrow_schema::DataType::Utf8, false),
        arrow_schema::Field::new("description", arrow_schema::DataType::Utf8, true),
        arrow_schema::Field::new(
            "dependencies",
            arrow_schema::DataType::List(Arc::new(arrow_schema::Field::new(
                "item",
                arrow_schema::DataType::Utf8,
                true,
            ))),
            true,
        ),
        arrow_schema::Field::new("project_name", arrow_schema::DataType::Utf8, false),
        arrow_schema::Field::new("chunk_id", arrow_schema::DataType::Utf8, false),
        arrow_schema::Field::new("content_hash", arrow_schema::DataType::Utf8, false),
        arrow_schema::Field::new(
            "embedding_model_version",
            arrow_schema::DataType::Utf8,
            false,
        ),
        arrow_schema::Field::new(
            "vector",
            arrow_schema::DataType::FixedSizeList(
                Arc::new(arrow_schema::Field::new(
                    "item",
                    arrow_schema::DataType::Float32,
                    true,
                )),
                dim as i32,
            ),
            false,
        ),
    ]));

    Ok(RecordBatch::try_new(
        schema,
        vec![
            Arc::new(crate_names),
            Arc::new(crate_paths),
            Arc::new(descriptions),
            Arc::new(dependencies) as ArrayRef,
            Arc::new(project_names),
            Arc::new(chunk_ids),
            Arc::new(content_hashes),
            Arc::new(model_versions),
            Arc::new(vectors),
        ],
    )?)
}

fn module_doc_chunks_to_batch(
    chunks: &[ModuleDocChunk],
    embeddings: Vec<Vec<f32>>,
    dim: usize,
) -> Result<RecordBatch, StoreError> {
    use arrow_array::builder::FixedSizeListBuilder;

    let file_paths: StringArray = chunks.iter().map(|c| Some(c.file_path.as_str())).collect();
    let module_names: StringArray = chunks
        .iter()
        .map(|c| Some(c.module_name.as_str()))
        .collect();
    let doc_contents: StringArray = chunks
        .iter()
        .map(|c| Some(c.doc_content.as_str()))
        .collect();
    let project_names: StringArray = chunks
        .iter()
        .map(|c| Some(c.project_name.as_str()))
        .collect();

    // New V1.1 fields
    let chunk_ids: StringArray = chunks.iter().map(|c| Some(c.chunk_id.as_str())).collect();
    let content_hashes: StringArray = chunks
        .iter()
        .map(|c| Some(c.content_hash.as_str()))
        .collect();
    let model_versions: StringArray = chunks
        .iter()
        .map(|c| Some(c.embedding_model_version.as_str()))
        .collect();

    let mut vector_builder =
        FixedSizeListBuilder::new(arrow_array::builder::Float32Builder::new(), dim as i32);

    for emb in &embeddings {
        vector_builder.values().append_slice(emb);
        vector_builder.append(true);
    }

    let vectors = vector_builder.finish();

    let schema = Arc::new(arrow_schema::Schema::new(vec![
        arrow_schema::Field::new("file_path", arrow_schema::DataType::Utf8, false),
        arrow_schema::Field::new("module_name", arrow_schema::DataType::Utf8, false),
        arrow_schema::Field::new("doc_content", arrow_schema::DataType::Utf8, false),
        arrow_schema::Field::new("project_name", arrow_schema::DataType::Utf8, false),
        arrow_schema::Field::new("chunk_id", arrow_schema::DataType::Utf8, false),
        arrow_schema::Field::new("content_hash", arrow_schema::DataType::Utf8, false),
        arrow_schema::Field::new(
            "embedding_model_version",
            arrow_schema::DataType::Utf8,
            false,
        ),
        arrow_schema::Field::new(
            "vector",
            arrow_schema::DataType::FixedSizeList(
                Arc::new(arrow_schema::Field::new(
                    "item",
                    arrow_schema::DataType::Float32,
                    true,
                )),
                dim as i32,
            ),
            false,
        ),
    ]));

    Ok(RecordBatch::try_new(
        schema,
        vec![
            Arc::new(file_paths),
            Arc::new(module_names),
            Arc::new(doc_contents),
            Arc::new(project_names),
            Arc::new(chunk_ids),
            Arc::new(content_hashes),
            Arc::new(model_versions),
            Arc::new(vectors),
        ],
    )?)
}

/// A2: Arrow batch for folder-summary chunks. Vec<String> fields are stored
/// as native `List<Utf8>` — same pattern as CrateChunk.dependencies.
fn folder_chunks_to_batch(
    chunks: &[FolderChunk],
    embeddings: Vec<Vec<f32>>,
    dim: usize,
) -> Result<RecordBatch, StoreError> {
    use arrow_array::builder::FixedSizeListBuilder;
    use arrow_array::{ArrayRef, ListArray};
    use arrow_buffer::OffsetBuffer;

    let folder_paths: StringArray = chunks
        .iter()
        .map(|c| Some(c.folder_path.as_str()))
        .collect();
    let project_names: StringArray = chunks
        .iter()
        .map(|c| Some(c.project_name.as_str()))
        .collect();
    let file_counts: UInt64Array = chunks.iter().map(|c| Some(c.file_count as u64)).collect();
    let summary_texts: StringArray = chunks
        .iter()
        .map(|c| Some(c.summary_text.as_str()))
        .collect();
    let chunk_ids: StringArray = chunks.iter().map(|c| Some(c.chunk_id.as_str())).collect();
    let content_hashes: StringArray = chunks
        .iter()
        .map(|c| Some(c.content_hash.as_str()))
        .collect();
    let model_versions: StringArray = chunks
        .iter()
        .map(|c| Some(c.embedding_model_version.as_str()))
        .collect();

    // Helper: build a List<Utf8> column from a slice-of-Vec<String>.
    fn list_of_strings(per_row: impl Iterator<Item = Vec<String>>) -> ListArray {
        let mut offsets = vec![0i32];
        let mut values: Vec<String> = Vec::new();
        for row in per_row {
            values.extend(row);
            offsets.push(values.len() as i32);
        }
        let refs: Vec<Option<&str>> = values.iter().map(|s| Some(s.as_str())).collect();
        let values_array: StringArray = refs.into_iter().collect();
        ListArray::new(
            Arc::new(arrow_schema::Field::new(
                "item",
                arrow_schema::DataType::Utf8,
                true,
            )),
            OffsetBuffer::new(offsets.into()),
            Arc::new(values_array),
            None,
        )
    }

    let languages = list_of_strings(chunks.iter().map(|c| c.languages.clone()));
    let key_types = list_of_strings(chunks.iter().map(|c| c.key_types.clone()));
    let key_functions = list_of_strings(chunks.iter().map(|c| c.key_functions.clone()));
    let subfolders = list_of_strings(chunks.iter().map(|c| c.subfolders.clone()));

    let mut vector_builder =
        FixedSizeListBuilder::new(arrow_array::builder::Float32Builder::new(), dim as i32);
    for emb in &embeddings {
        vector_builder.values().append_slice(emb);
        vector_builder.append(true);
    }
    let vectors = vector_builder.finish();

    let list_field = || {
        arrow_schema::DataType::List(Arc::new(arrow_schema::Field::new(
            "item",
            arrow_schema::DataType::Utf8,
            true,
        )))
    };

    let schema = Arc::new(arrow_schema::Schema::new(vec![
        arrow_schema::Field::new("folder_path", arrow_schema::DataType::Utf8, false),
        arrow_schema::Field::new("project_name", arrow_schema::DataType::Utf8, false),
        arrow_schema::Field::new("file_count", arrow_schema::DataType::UInt64, false),
        arrow_schema::Field::new("languages", list_field(), true),
        arrow_schema::Field::new("key_types", list_field(), true),
        arrow_schema::Field::new("key_functions", list_field(), true),
        arrow_schema::Field::new("subfolders", list_field(), true),
        arrow_schema::Field::new("summary_text", arrow_schema::DataType::Utf8, false),
        arrow_schema::Field::new("chunk_id", arrow_schema::DataType::Utf8, false),
        arrow_schema::Field::new("content_hash", arrow_schema::DataType::Utf8, false),
        arrow_schema::Field::new(
            "embedding_model_version",
            arrow_schema::DataType::Utf8,
            false,
        ),
        arrow_schema::Field::new(
            "vector",
            arrow_schema::DataType::FixedSizeList(
                Arc::new(arrow_schema::Field::new(
                    "item",
                    arrow_schema::DataType::Float32,
                    true,
                )),
                dim as i32,
            ),
            false,
        ),
    ]));

    Ok(RecordBatch::try_new(
        schema,
        vec![
            Arc::new(folder_paths),
            Arc::new(project_names),
            Arc::new(file_counts),
            Arc::new(languages) as ArrayRef,
            Arc::new(key_types) as ArrayRef,
            Arc::new(key_functions) as ArrayRef,
            Arc::new(subfolders) as ArrayRef,
            Arc::new(summary_texts),
            Arc::new(chunk_ids),
            Arc::new(content_hashes),
            Arc::new(model_versions),
            Arc::new(vectors),
        ],
    )?)
}

async fn batches_to_folder_chunks(
    stream: impl futures::Stream<Item = Result<RecordBatch, lancedb::Error>> + Unpin,
) -> Result<Vec<(FolderChunk, f32)>, StoreError> {
    use futures::TryStreamExt;
    stream
        .map_err(StoreError::from)
        .try_fold(Vec::new(), |mut acc, batch| async move {
            acc.extend(extract_folder_chunks_from_batch(&batch, "_distance")?);
            Ok(acc)
        })
        .await
}

async fn batches_to_folder_chunks_hybrid(
    stream: impl futures::Stream<Item = Result<RecordBatch, lancedb::Error>> + Unpin,
) -> Result<Vec<(FolderChunk, f32)>, StoreError> {
    use futures::TryStreamExt;
    stream
        .map_err(StoreError::from)
        .try_fold(Vec::new(), |mut acc, batch| async move {
            acc.extend(extract_folder_chunks_from_batch(
                &batch,
                "_relevance_score",
            )?);
            Ok(acc)
        })
        .await
}

fn extract_folder_chunks_from_batch(
    batch: &RecordBatch,
    score_column: &str,
) -> Result<Vec<(FolderChunk, f32)>, StoreError> {
    use arrow_array::ListArray;

    let col = |name: &str| -> Result<&StringArray, StoreError> {
        batch
            .column_by_name(name)
            .and_then(|c| c.as_any().downcast_ref::<StringArray>())
            .ok_or_else(|| StoreError::SchemaMismatch(name.into()))
    };

    let folder_paths = col("folder_path")?;
    let project_names = col("project_name")?;
    let summary_texts = col("summary_text")?;
    let chunk_ids = col("chunk_id")?;
    let content_hashes = col("content_hash")?;
    let model_versions = col("embedding_model_version")?;

    let file_counts = batch
        .column_by_name("file_count")
        .and_then(|c| c.as_any().downcast_ref::<UInt64Array>())
        .ok_or_else(|| StoreError::SchemaMismatch("file_count".into()))?;

    let list = |name: &str| -> Option<&ListArray> {
        batch
            .column_by_name(name)
            .and_then(|c| c.as_any().downcast_ref::<ListArray>())
    };
    let languages_list = list("languages");
    let key_types_list = list("key_types");
    let key_functions_list = list("key_functions");
    let subfolders_list = list("subfolders");

    let scores = batch
        .column_by_name(score_column)
        .and_then(|c| c.as_any().downcast_ref::<Float32Array>());

    let extract_list = |arr: Option<&ListArray>, i: usize| -> Vec<String> {
        arr.filter(|a| !a.is_null(i))
            .map(|a| {
                let v = a.value(i);
                v.as_any()
                    .downcast_ref::<StringArray>()
                    .map(|sa| {
                        (0..sa.len())
                            .filter_map(|j| {
                                if sa.is_null(j) {
                                    None
                                } else {
                                    Some(sa.value(j).to_string())
                                }
                            })
                            .collect()
                    })
                    .unwrap_or_default()
            })
            .unwrap_or_default()
    };

    let rows = (0..batch.num_rows())
        .map(|i| {
            let chunk = FolderChunk {
                folder_path: folder_paths.value(i).to_string(),
                project_name: project_names.value(i).to_string(),
                file_count: file_counts.value(i) as usize,
                languages: extract_list(languages_list, i),
                key_types: extract_list(key_types_list, i),
                key_functions: extract_list(key_functions_list, i),
                subfolders: extract_list(subfolders_list, i),
                summary_text: summary_texts.value(i).to_string(),
                chunk_id: chunk_ids.value(i).to_string(),
                content_hash: content_hashes.value(i).to_string(),
                embedding_model_version: model_versions.value(i).to_string(),
            };
            let score = scores.map(|d| d.value(i)).unwrap_or(0.0);
            (chunk, score)
        })
        .collect();
    Ok(rows)
}

fn file_chunks_to_batch(
    chunks: &[FileChunk],
    embeddings: Vec<Vec<f32>>,
    dim: usize,
) -> Result<RecordBatch, StoreError> {
    use arrow_array::builder::FixedSizeListBuilder;
    use arrow_array::{ArrayRef, ListArray};
    use arrow_buffer::OffsetBuffer;

    let file_paths: StringArray = chunks.iter().map(|c| Some(c.file_path.as_str())).collect();
    let project_names: StringArray = chunks
        .iter()
        .map(|c| Some(c.project_name.as_str()))
        .collect();
    let languages: StringArray = chunks.iter().map(|c| Some(c.language.as_str())).collect();
    let purposes: StringArray = chunks.iter().map(|c| c.purpose.as_deref()).collect();
    let summary_texts: StringArray = chunks
        .iter()
        .map(|c| Some(c.summary_text.as_str()))
        .collect();
    let chunk_ids: StringArray = chunks.iter().map(|c| Some(c.chunk_id.as_str())).collect();
    let content_hashes: StringArray = chunks
        .iter()
        .map(|c| Some(c.content_hash.as_str()))
        .collect();
    let model_versions: StringArray = chunks
        .iter()
        .map(|c| Some(c.embedding_model_version.as_str()))
        .collect();

    fn list_of_strings(per_row: impl Iterator<Item = Vec<String>>) -> ListArray {
        let mut offsets = vec![0i32];
        let mut values: Vec<String> = Vec::new();
        for row in per_row {
            values.extend(row);
            offsets.push(values.len() as i32);
        }
        let refs: Vec<Option<&str>> = values.iter().map(|s| Some(s.as_str())).collect();
        let values_array: StringArray = refs.into_iter().collect();
        ListArray::new(
            Arc::new(arrow_schema::Field::new(
                "item",
                arrow_schema::DataType::Utf8,
                true,
            )),
            OffsetBuffer::new(offsets.into()),
            Arc::new(values_array),
            None,
        )
    }

    let exports = list_of_strings(chunks.iter().map(|c| c.exports.clone()));
    let imports = list_of_strings(chunks.iter().map(|c| c.imports.clone()));

    let mut vector_builder =
        FixedSizeListBuilder::new(arrow_array::builder::Float32Builder::new(), dim as i32);
    for emb in &embeddings {
        vector_builder.values().append_slice(emb);
        vector_builder.append(true);
    }
    let vectors = vector_builder.finish();

    let list_field = || {
        arrow_schema::DataType::List(Arc::new(arrow_schema::Field::new(
            "item",
            arrow_schema::DataType::Utf8,
            true,
        )))
    };

    let schema = Arc::new(arrow_schema::Schema::new(vec![
        arrow_schema::Field::new("file_path", arrow_schema::DataType::Utf8, false),
        arrow_schema::Field::new("project_name", arrow_schema::DataType::Utf8, false),
        arrow_schema::Field::new("language", arrow_schema::DataType::Utf8, false),
        arrow_schema::Field::new("exports", list_field(), true),
        arrow_schema::Field::new("imports", list_field(), true),
        arrow_schema::Field::new("purpose", arrow_schema::DataType::Utf8, true),
        arrow_schema::Field::new("summary_text", arrow_schema::DataType::Utf8, false),
        arrow_schema::Field::new("chunk_id", arrow_schema::DataType::Utf8, false),
        arrow_schema::Field::new("content_hash", arrow_schema::DataType::Utf8, false),
        arrow_schema::Field::new(
            "embedding_model_version",
            arrow_schema::DataType::Utf8,
            false,
        ),
        arrow_schema::Field::new(
            "vector",
            arrow_schema::DataType::FixedSizeList(
                Arc::new(arrow_schema::Field::new(
                    "item",
                    arrow_schema::DataType::Float32,
                    true,
                )),
                dim as i32,
            ),
            false,
        ),
    ]));

    Ok(RecordBatch::try_new(
        schema,
        vec![
            Arc::new(file_paths),
            Arc::new(project_names),
            Arc::new(languages),
            Arc::new(exports) as ArrayRef,
            Arc::new(imports) as ArrayRef,
            Arc::new(purposes),
            Arc::new(summary_texts),
            Arc::new(chunk_ids),
            Arc::new(content_hashes),
            Arc::new(model_versions),
            Arc::new(vectors),
        ],
    )?)
}

async fn batches_to_file_chunks(
    stream: impl futures::Stream<Item = Result<RecordBatch, lancedb::Error>> + Unpin,
) -> Result<Vec<(FileChunk, f32)>, StoreError> {
    use futures::TryStreamExt;
    stream
        .map_err(StoreError::from)
        .try_fold(Vec::new(), |mut acc, batch| async move {
            acc.extend(extract_file_chunks_from_batch(&batch, "_distance")?);
            Ok(acc)
        })
        .await
}

async fn batches_to_file_chunks_hybrid(
    stream: impl futures::Stream<Item = Result<RecordBatch, lancedb::Error>> + Unpin,
) -> Result<Vec<(FileChunk, f32)>, StoreError> {
    use futures::TryStreamExt;
    stream
        .map_err(StoreError::from)
        .try_fold(Vec::new(), |mut acc, batch| async move {
            acc.extend(extract_file_chunks_from_batch(&batch, "_relevance_score")?);
            Ok(acc)
        })
        .await
}

fn extract_file_chunks_from_batch(
    batch: &RecordBatch,
    score_column: &str,
) -> Result<Vec<(FileChunk, f32)>, StoreError> {
    use arrow_array::ListArray;

    let col = |name: &str| -> Result<&StringArray, StoreError> {
        batch
            .column_by_name(name)
            .and_then(|c| c.as_any().downcast_ref::<StringArray>())
            .ok_or_else(|| StoreError::SchemaMismatch(name.into()))
    };

    let file_paths = col("file_path")?;
    let project_names = col("project_name")?;
    let languages = col("language")?;
    let summary_texts = col("summary_text")?;
    let chunk_ids = col("chunk_id")?;
    let content_hashes = col("content_hash")?;
    let model_versions = col("embedding_model_version")?;

    let purposes: Option<&StringArray> = batch
        .column_by_name("purpose")
        .and_then(|c| c.as_any().downcast_ref::<StringArray>());

    let list = |name: &str| -> Option<&ListArray> {
        batch
            .column_by_name(name)
            .and_then(|c| c.as_any().downcast_ref::<ListArray>())
    };
    let exports_list = list("exports");
    let imports_list = list("imports");

    let scores = batch
        .column_by_name(score_column)
        .and_then(|c| c.as_any().downcast_ref::<Float32Array>());

    let extract_list = |arr: Option<&ListArray>, i: usize| -> Vec<String> {
        arr.filter(|a| !a.is_null(i))
            .map(|a| {
                let v = a.value(i);
                v.as_any()
                    .downcast_ref::<StringArray>()
                    .map(|sa| {
                        (0..sa.len())
                            .filter_map(|j| {
                                if sa.is_null(j) {
                                    None
                                } else {
                                    Some(sa.value(j).to_string())
                                }
                            })
                            .collect()
                    })
                    .unwrap_or_default()
            })
            .unwrap_or_default()
    };

    let rows = (0..batch.num_rows())
        .map(|i| {
            let purpose = purposes
                .filter(|a| !a.is_null(i))
                .map(|a| a.value(i).to_string());
            let chunk = FileChunk {
                file_path: file_paths.value(i).to_string(),
                project_name: project_names.value(i).to_string(),
                language: languages.value(i).to_string(),
                exports: extract_list(exports_list, i),
                imports: extract_list(imports_list, i),
                purpose,
                summary_text: summary_texts.value(i).to_string(),
                chunk_id: chunk_ids.value(i).to_string(),
                content_hash: content_hashes.value(i).to_string(),
                embedding_model_version: model_versions.value(i).to_string(),
            };
            let score = scores.map(|d| d.value(i)).unwrap_or(0.0);
            (chunk, score)
        })
        .collect();
    Ok(rows)
}

async fn batches_to_code_chunks(
    stream: impl futures::Stream<Item = Result<RecordBatch, lancedb::Error>> + Unpin,
) -> Result<Vec<(CodeChunk, f32)>, StoreError> {
    use futures::TryStreamExt;

    stream
        .map_err(StoreError::from)
        .try_fold(Vec::new(), |mut acc, batch| async move {
            acc.extend(extract_code_chunks_from_batch(&batch)?);
            Ok(acc)
        })
        .await
}

async fn batches_to_code_chunks_hybrid(
    stream: impl futures::Stream<Item = Result<RecordBatch, lancedb::Error>> + Unpin,
) -> Result<Vec<(CodeChunk, f32)>, StoreError> {
    use futures::TryStreamExt;

    stream
        .map_err(StoreError::from)
        .try_fold(Vec::new(), |mut acc, batch| async move {
            acc.extend(extract_code_chunks_from_batch_with_score(
                &batch,
                "_relevance_score",
            )?);
            Ok(acc)
        })
        .await
}

fn extract_code_chunks_from_batch(
    batch: &RecordBatch,
) -> Result<Vec<(CodeChunk, f32)>, StoreError> {
    extract_code_chunks_from_batch_with_score(batch, "_distance")
}

fn extract_code_chunks_from_batch_with_score(
    batch: &RecordBatch,
    score_column: &str,
) -> Result<Vec<(CodeChunk, f32)>, StoreError> {
    let col = |name: &str| -> Result<&StringArray, StoreError> {
        batch
            .column_by_name(name)
            .and_then(|c| c.as_any().downcast_ref::<StringArray>())
            .ok_or_else(|| StoreError::SchemaMismatch(name.into()))
    };

    let file_paths = col("file_path")?;
    let languages = col("language")?;
    let identifiers = col("identifier")?;
    let node_types = col("node_type")?;
    let code_contents = col("code_content")?;
    let chunk_ids = col("chunk_id")?;
    let content_hashes = col("content_hash")?;
    let model_versions = col("embedding_model_version")?;

    let start_lines = batch
        .column_by_name("start_line")
        .and_then(|c| c.as_any().downcast_ref::<UInt64Array>())
        .ok_or_else(|| StoreError::SchemaMismatch("start_line".into()))?;

    let project_names = col("project_name")?;

    // Optional columns
    let docstrings = batch
        .column_by_name("docstring")
        .and_then(|c| c.as_any().downcast_ref::<StringArray>());

    let signatures = batch
        .column_by_name("signature")
        .and_then(|c| c.as_any().downcast_ref::<StringArray>());

    let scores = batch
        .column_by_name(score_column)
        .and_then(|c| c.as_any().downcast_ref::<Float32Array>());

    let nullable_string = |arr: Option<&StringArray>, i: usize| -> Option<String> {
        arr.filter(|a| !a.is_null(i))
            .map(|a| a.value(i).to_string())
    };

    let chunks = (0..batch.num_rows())
        .map(|i| {
            let chunk = CodeChunk {
                file_path: file_paths.value(i).to_string(),
                language: languages.value(i).to_string(),
                identifier: identifiers.value(i).to_string(),
                node_type: node_types.value(i).to_string(),
                code_content: code_contents.value(i).to_string(),
                start_line: start_lines.value(i) as usize,
                project_name: project_names.value(i).to_string(),
                docstring: nullable_string(docstrings, i),
                signature: nullable_string(signatures, i),
                chunk_id: chunk_ids.value(i).to_string(),
                content_hash: content_hashes.value(i).to_string(),
                embedding_model_version: model_versions.value(i).to_string(),
            };
            let score = scores.map(|d| d.value(i)).unwrap_or(0.0);
            (chunk, score)
        })
        .collect();

    Ok(chunks)
}

async fn batches_to_readme_chunks(
    stream: impl futures::Stream<Item = Result<RecordBatch, lancedb::Error>> + Unpin,
) -> Result<Vec<(ReadmeChunk, f32)>, StoreError> {
    use futures::TryStreamExt;

    stream
        .map_err(StoreError::from)
        .try_fold(Vec::new(), |mut acc, batch| async move {
            acc.extend(extract_readme_chunks_from_batch(&batch)?);
            Ok(acc)
        })
        .await
}

async fn batches_to_readme_chunks_hybrid(
    stream: impl futures::Stream<Item = Result<RecordBatch, lancedb::Error>> + Unpin,
) -> Result<Vec<(ReadmeChunk, f32)>, StoreError> {
    use futures::TryStreamExt;

    stream
        .map_err(StoreError::from)
        .try_fold(Vec::new(), |mut acc, batch| async move {
            acc.extend(extract_readme_chunks_from_batch_with_score(
                &batch,
                "_relevance_score",
            )?);
            Ok(acc)
        })
        .await
}

fn extract_readme_chunks_from_batch(
    batch: &RecordBatch,
) -> Result<Vec<(ReadmeChunk, f32)>, StoreError> {
    extract_readme_chunks_from_batch_with_score(batch, "_distance")
}

fn extract_readme_chunks_from_batch_with_score(
    batch: &RecordBatch,
    score_column: &str,
) -> Result<Vec<(ReadmeChunk, f32)>, StoreError> {
    let col = |name: &str| -> Result<&StringArray, StoreError> {
        batch
            .column_by_name(name)
            .and_then(|c| c.as_any().downcast_ref::<StringArray>())
            .ok_or_else(|| StoreError::SchemaMismatch(name.into()))
    };

    let file_paths = col("file_path")?;
    let project_names = col("project_name")?;
    let contents = col("content")?;
    let chunk_ids = col("chunk_id")?;
    let content_hashes = col("content_hash")?;
    let model_versions = col("embedding_model_version")?;

    let scores = batch
        .column_by_name(score_column)
        .and_then(|c| c.as_any().downcast_ref::<Float32Array>());

    let chunks = (0..batch.num_rows())
        .map(|i| {
            let chunk = ReadmeChunk {
                file_path: file_paths.value(i).to_string(),
                project_name: project_names.value(i).to_string(),
                content: contents.value(i).to_string(),
                chunk_id: chunk_ids.value(i).to_string(),
                content_hash: content_hashes.value(i).to_string(),
                embedding_model_version: model_versions.value(i).to_string(),
            };
            let score = scores.map(|d| d.value(i)).unwrap_or(0.0);
            (chunk, score)
        })
        .collect();

    Ok(chunks)
}

async fn batches_to_crate_chunks(
    stream: impl futures::Stream<Item = Result<RecordBatch, lancedb::Error>> + Unpin,
) -> Result<Vec<(CrateChunk, f32)>, StoreError> {
    use futures::TryStreamExt;

    stream
        .map_err(StoreError::from)
        .try_fold(Vec::new(), |mut acc, batch| async move {
            acc.extend(extract_crate_chunks_from_batch(&batch)?);
            Ok(acc)
        })
        .await
}

async fn batches_to_crate_chunks_hybrid(
    stream: impl futures::Stream<Item = Result<RecordBatch, lancedb::Error>> + Unpin,
) -> Result<Vec<(CrateChunk, f32)>, StoreError> {
    use futures::TryStreamExt;

    stream
        .map_err(StoreError::from)
        .try_fold(Vec::new(), |mut acc, batch| async move {
            acc.extend(extract_crate_chunks_from_batch_with_score(
                &batch,
                "_relevance_score",
            )?);
            Ok(acc)
        })
        .await
}

fn extract_crate_chunks_from_batch(
    batch: &RecordBatch,
) -> Result<Vec<(CrateChunk, f32)>, StoreError> {
    extract_crate_chunks_from_batch_with_score(batch, "_distance")
}

fn extract_crate_chunks_from_batch_with_score(
    batch: &RecordBatch,
    score_column: &str,
) -> Result<Vec<(CrateChunk, f32)>, StoreError> {
    use arrow_array::ListArray;

    let col = |name: &str| -> Result<&StringArray, StoreError> {
        batch
            .column_by_name(name)
            .and_then(|c| c.as_any().downcast_ref::<StringArray>())
            .ok_or_else(|| StoreError::SchemaMismatch(name.into()))
    };

    let crate_names = col("crate_name")?;
    let crate_paths = col("crate_path")?;
    let project_names = col("project_name")?;
    let chunk_ids = col("chunk_id")?;
    let content_hashes = col("content_hash")?;
    let model_versions = col("embedding_model_version")?;

    let descriptions = batch
        .column_by_name("description")
        .and_then(|c| c.as_any().downcast_ref::<StringArray>());

    // Dependencies is now List<Utf8>
    let dependencies_list = batch
        .column_by_name("dependencies")
        .and_then(|c| c.as_any().downcast_ref::<ListArray>());

    let scores = batch
        .column_by_name(score_column)
        .and_then(|c| c.as_any().downcast_ref::<Float32Array>());

    let nullable_string = |arr: Option<&StringArray>, i: usize| -> Option<String> {
        arr.filter(|a| !a.is_null(i))
            .map(|a| a.value(i).to_string())
    };

    let chunks = (0..batch.num_rows())
        .map(|i| {
            // Extract dependencies from ListArray
            let deps = dependencies_list
                .filter(|arr| !arr.is_null(i))
                .map(|arr| {
                    let list_value = arr.value(i);
                    let string_arr = list_value.as_any().downcast_ref::<StringArray>();
                    string_arr
                        .map(|sa| {
                            (0..sa.len())
                                .filter_map(|j| {
                                    if sa.is_null(j) {
                                        None
                                    } else {
                                        Some(sa.value(j).to_string())
                                    }
                                })
                                .collect()
                        })
                        .unwrap_or_default()
                })
                .unwrap_or_default();

            let chunk = CrateChunk {
                crate_name: crate_names.value(i).to_string(),
                crate_path: crate_paths.value(i).to_string(),
                description: nullable_string(descriptions, i),
                dependencies: deps,
                project_name: project_names.value(i).to_string(),
                chunk_id: chunk_ids.value(i).to_string(),
                content_hash: content_hashes.value(i).to_string(),
                embedding_model_version: model_versions.value(i).to_string(),
            };
            let score = scores.map(|d| d.value(i)).unwrap_or(0.0);
            (chunk, score)
        })
        .collect();

    Ok(chunks)
}

async fn batches_to_module_doc_chunks(
    stream: impl futures::Stream<Item = Result<RecordBatch, lancedb::Error>> + Unpin,
) -> Result<Vec<(ModuleDocChunk, f32)>, StoreError> {
    use futures::TryStreamExt;

    stream
        .map_err(StoreError::from)
        .try_fold(Vec::new(), |mut acc, batch| async move {
            acc.extend(extract_module_doc_chunks_from_batch(&batch)?);
            Ok(acc)
        })
        .await
}

async fn batches_to_module_doc_chunks_hybrid(
    stream: impl futures::Stream<Item = Result<RecordBatch, lancedb::Error>> + Unpin,
) -> Result<Vec<(ModuleDocChunk, f32)>, StoreError> {
    use futures::TryStreamExt;

    stream
        .map_err(StoreError::from)
        .try_fold(Vec::new(), |mut acc, batch| async move {
            acc.extend(extract_module_doc_chunks_from_batch_with_score(
                &batch,
                "_relevance_score",
            )?);
            Ok(acc)
        })
        .await
}

fn extract_module_doc_chunks_from_batch(
    batch: &RecordBatch,
) -> Result<Vec<(ModuleDocChunk, f32)>, StoreError> {
    extract_module_doc_chunks_from_batch_with_score(batch, "_distance")
}

fn extract_module_doc_chunks_from_batch_with_score(
    batch: &RecordBatch,
    score_column: &str,
) -> Result<Vec<(ModuleDocChunk, f32)>, StoreError> {
    let col = |name: &str| -> Result<&StringArray, StoreError> {
        batch
            .column_by_name(name)
            .and_then(|c| c.as_any().downcast_ref::<StringArray>())
            .ok_or_else(|| StoreError::SchemaMismatch(name.into()))
    };

    let file_paths = col("file_path")?;
    let module_names = col("module_name")?;
    let doc_contents = col("doc_content")?;
    let project_names = col("project_name")?;
    let chunk_ids = col("chunk_id")?;
    let content_hashes = col("content_hash")?;
    let model_versions = col("embedding_model_version")?;

    let scores = batch
        .column_by_name(score_column)
        .and_then(|c| c.as_any().downcast_ref::<Float32Array>());

    let chunks = (0..batch.num_rows())
        .map(|i| {
            let chunk = ModuleDocChunk {
                file_path: file_paths.value(i).to_string(),
                module_name: module_names.value(i).to_string(),
                doc_content: doc_contents.value(i).to_string(),
                project_name: project_names.value(i).to_string(),
                chunk_id: chunk_ids.value(i).to_string(),
                content_hash: content_hashes.value(i).to_string(),
                embedding_model_version: model_versions.value(i).to_string(),
            };
            let score = scores.map(|d| d.value(i)).unwrap_or(0.0);
            (chunk, score)
        })
        .collect();

    Ok(chunks)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Schema-version sentinel -------------------------------------------

    #[test]
    fn schema_version_check_passes_on_fresh_dir() {
        let tmp = tempfile::tempdir().unwrap();
        // No sidecar yet → compat check is a no-op.
        check_schema_version_compat(tmp.path()).expect("fresh dir must be accepted");
    }

    #[test]
    fn schema_version_check_passes_when_sidecar_matches() {
        let tmp = tempfile::tempdir().unwrap();
        write_schema_version(tmp.path()).expect("write sidecar");
        check_schema_version_compat(tmp.path()).expect("matching sidecar must be accepted");
    }

    #[test]
    fn schema_version_check_rejects_older_version() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join(SCHEMA_VERSION_FILE), "0").unwrap();
        let err = check_schema_version_compat(tmp.path()).unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("schema v0") && msg.contains(&format!("v{}", SCHEMA_VERSION)),
            "error should mention both versions; got: {msg}"
        );
        assert!(
            msg.contains("rm -rf") && msg.contains("--full"),
            "error should give the recovery command; got: {msg}"
        );
    }

    #[test]
    fn schema_version_check_rejects_garbage_sidecar() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join(SCHEMA_VERSION_FILE), "not-a-number").unwrap();
        let err = check_schema_version_compat(tmp.path()).unwrap_err();
        assert!(matches!(err, StoreError::SchemaMismatch(_)));
    }

    #[test]
    fn write_schema_version_is_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        write_schema_version(tmp.path()).unwrap();
        let mtime1 = std::fs::metadata(tmp.path().join(SCHEMA_VERSION_FILE))
            .unwrap()
            .modified()
            .unwrap();
        // Second call with the matching version must be a no-op (no rewrite).
        write_schema_version(tmp.path()).unwrap();
        let mtime2 = std::fs::metadata(tmp.path().join(SCHEMA_VERSION_FILE))
            .unwrap()
            .modified()
            .unwrap();
        assert_eq!(mtime1, mtime2, "second write must not touch the file");
    }

    fn sample_code_chunk() -> CodeChunk {
        CodeChunk {
            file_path: "/test/main.rs".into(),
            language: "rust".into(),
            identifier: "test_func".into(),
            node_type: "function_item".into(),
            code_content: "fn test_func() {}".into(),
            start_line: 1,
            project_name: "test_project".into(),
            docstring: Some("A test function".into()),
            signature: Some("fn test_func()".into()),
            chunk_id: "test-uuid-1234".into(),
            content_hash: "abc123".into(),
            embedding_model_version: "BGESmallENV15_384".into(),
        }
    }

    fn sample_readme_chunk() -> ReadmeChunk {
        ReadmeChunk {
            file_path: "/test/README.md".into(),
            project_name: "test_project".into(),
            content: "# Test Project\nThis is a test.".into(),
            chunk_id: "test-uuid-5678".into(),
            content_hash: "def456".into(),
            embedding_model_version: "BGESmallENV15_384".into(),
        }
    }

    fn fake_embedding(dim: usize) -> Vec<f32> {
        vec![0.1; dim]
    }

    #[test]
    fn test_code_chunks_to_batch() {
        let chunks = vec![sample_code_chunk()];
        let embeddings = vec![fake_embedding(384)];
        let sig_embeddings = vec![Some(fake_embedding(384))];

        let batch = code_chunks_to_batch(&chunks, embeddings, sig_embeddings, 384).unwrap();

        assert_eq!(batch.num_rows(), 1);
        // 13 text/int fields + body vector + signature_vector = 15
        assert_eq!(batch.num_columns(), 15);
    }

    #[test]
    fn test_code_chunks_to_batch_signature_null() {
        // Chunks without signatures get null in signature_vector column.
        let chunks = vec![sample_code_chunk()];
        let embeddings = vec![fake_embedding(384)];
        let sig_embeddings: Vec<Option<Vec<f32>>> = vec![None];

        let batch = code_chunks_to_batch(&chunks, embeddings, sig_embeddings, 384).unwrap();

        let sig_col = batch.column_by_name("signature_vector").unwrap();
        assert!(sig_col.is_null(0));
    }

    #[test]
    fn test_readme_chunks_to_batch() {
        let chunks = vec![sample_readme_chunk()];
        let embeddings = vec![fake_embedding(384)];

        let batch = readme_chunks_to_batch(&chunks, embeddings, 384).unwrap();

        assert_eq!(batch.num_rows(), 1);
        assert_eq!(batch.num_columns(), 7); // 6 fields + vector
    }

    #[test]
    fn test_code_batch_preserves_data() {
        let chunk = sample_code_chunk();
        let chunks = vec![chunk.clone()];
        let embeddings = vec![fake_embedding(384)];
        let sig_embeddings = vec![Some(fake_embedding(384))];

        let batch = code_chunks_to_batch(&chunks, embeddings, sig_embeddings, 384).unwrap();

        let identifiers = batch
            .column_by_name("identifier")
            .unwrap()
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();

        assert_eq!(identifiers.value(0), chunk.identifier);
    }

    #[tokio::test]
    #[ignore = "requires filesystem, run with --ignored"]
    async fn test_vector_store_roundtrip() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.lance");

        let store = VectorStore::new(db_path.to_str().unwrap(), 384)
            .await
            .unwrap();

        let chunks = vec![sample_code_chunk()];
        let embeddings = vec![fake_embedding(384)];
        let sig_embeddings = vec![Some(fake_embedding(384))];

        let count = store
            .upsert_code_chunks(&chunks, embeddings, sig_embeddings)
            .await
            .unwrap();
        assert_eq!(count, 1);

        let results = store.search_code(&fake_embedding(384), 10).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0.identifier, "test_func");
    }

    fn sample_folder_chunk() -> FolderChunk {
        FolderChunk {
            folder_path: "code-rag/crates/code-rag-engine/src".into(),
            project_name: "code-rag".into(),
            file_count: 6,
            languages: vec!["rust".into()],
            key_types: vec!["ArmPolicy".into(), "RetrievalConfig".into()],
            key_functions: vec!["arm_policy".into(), "classify".into()],
            subfolders: vec!["text".into()],
            summary_text: "Folder: code-rag/crates/code-rag-engine/src (module: src)\nContains: 6 files (rust)\nKey types: ArmPolicy, RetrievalConfig\nKey functions: arm_policy, classify\nSubfolders: text".into(),
            chunk_id: "fold-1".into(),
            content_hash: "hash-1".into(),
            embedding_model_version: "BGESmallENV15_384".into(),
        }
    }

    #[test]
    fn test_folder_chunks_to_batch_roundtrip() {
        let chunks = vec![sample_folder_chunk()];
        let embeddings = vec![fake_embedding(384)];
        let batch = folder_chunks_to_batch(&chunks, embeddings, 384).unwrap();
        assert_eq!(batch.num_rows(), 1);
        // 7 scalar + 4 list + 1 vector = 12 columns
        assert_eq!(batch.num_columns(), 12);

        let roundtripped = extract_folder_chunks_from_batch(&batch, "_distance").unwrap();
        assert_eq!(roundtripped.len(), 1);
        let (got, _score) = &roundtripped[0];
        assert_eq!(got.folder_path, chunks[0].folder_path);
        assert_eq!(got.project_name, chunks[0].project_name);
        assert_eq!(got.file_count, 6);
        assert_eq!(got.languages, chunks[0].languages);
        assert_eq!(got.key_types, chunks[0].key_types);
        assert_eq!(got.key_functions, chunks[0].key_functions);
        assert_eq!(got.subfolders, chunks[0].subfolders);
        assert_eq!(got.summary_text, chunks[0].summary_text);
        assert_eq!(got.chunk_id, chunks[0].chunk_id);
    }

    #[test]
    fn test_folder_chunks_to_batch_empty_lists_roundtrip() {
        let mut chunk = sample_folder_chunk();
        chunk.languages.clear();
        chunk.key_types.clear();
        chunk.key_functions.clear();
        chunk.subfolders.clear();
        let batch =
            folder_chunks_to_batch(&[chunk.clone()], vec![fake_embedding(384)], 384).unwrap();
        let got = &extract_folder_chunks_from_batch(&batch, "_distance").unwrap()[0].0;
        assert!(got.languages.is_empty());
        assert!(got.key_types.is_empty());
        assert!(got.key_functions.is_empty());
        assert!(got.subfolders.is_empty());
    }

    #[test]
    fn test_call_edges_to_batch_roundtrip() {
        let edge = CallEdge {
            edge_id: "edge1".into(),
            caller_chunk_id: "caller1".into(),
            callee_chunk_id: "callee1".into(),
            caller_identifier: "foo".into(),
            callee_identifier: "bar".into(),
            caller_file: "src/a.rs".into(),
            callee_file: "src/b.rs".into(),
            project_name: "test".into(),
            resolution_tier: 1,
        };

        let batch = call_edges_to_batch(&[edge]).unwrap();
        assert_eq!(batch.num_rows(), 1);
        assert_eq!(batch.num_columns(), 9);

        let edges = extract_call_edges_from_batch(&batch).unwrap();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].edge_id, "edge1");
        assert_eq!(edges[0].caller_identifier, "foo");
        assert_eq!(edges[0].callee_identifier, "bar");
        assert_eq!(edges[0].resolution_tier, 1);
    }

    #[tokio::test]
    #[ignore = "requires filesystem, run with --ignored"]
    async fn test_call_edges_upsert_and_query() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.lance");
        let store = VectorStore::new(db_path.to_str().unwrap(), 384)
            .await
            .unwrap();

        let edges = vec![
            CallEdge {
                edge_id: "e1".into(),
                caller_chunk_id: "c_foo".into(),
                callee_chunk_id: "c_bar".into(),
                caller_identifier: "foo".into(),
                callee_identifier: "bar".into(),
                caller_file: "src/a.rs".into(),
                callee_file: "src/a.rs".into(),
                project_name: "proj".into(),
                resolution_tier: 1,
            },
            CallEdge {
                edge_id: "e2".into(),
                caller_chunk_id: "c_foo".into(),
                callee_chunk_id: "c_baz".into(),
                caller_identifier: "foo".into(),
                callee_identifier: "baz".into(),
                caller_file: "src/a.rs".into(),
                callee_file: "src/b.rs".into(),
                project_name: "proj".into(),
                resolution_tier: 2,
            },
        ];

        let count = store.upsert_call_edges(&edges).await.unwrap();
        assert_eq!(count, 2);

        // Query callers of bar → foo
        let callers = store.get_callers("c_bar", Some("proj")).await.unwrap();
        assert_eq!(callers.len(), 1);
        assert_eq!(callers[0].caller_identifier, "foo");

        // Query callees of foo → bar, baz
        let callees = store.get_callees("c_foo", Some("proj")).await.unwrap();
        assert_eq!(callees.len(), 2);

        // Query all edges for project
        let all = store.get_all_edges("proj").await.unwrap();
        assert_eq!(all.len(), 2);

        // Delete by project
        store.delete_edges_by_project("proj").await.unwrap();
        let after_delete = store.get_all_edges("proj").await.unwrap();
        assert_eq!(after_delete.len(), 0);
    }

    #[tokio::test]
    #[ignore = "requires filesystem, run with --ignored"]
    async fn test_get_chunks_by_ids() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.lance");
        let store = VectorStore::new(db_path.to_str().unwrap(), 384)
            .await
            .unwrap();

        let mut chunk = sample_code_chunk();
        chunk.chunk_id = "known_id".into();
        let chunks = vec![chunk];
        let embeddings = vec![fake_embedding(384)];
        let sig_embeddings = vec![Some(fake_embedding(384))];

        store
            .upsert_code_chunks(&chunks, embeddings, sig_embeddings)
            .await
            .unwrap();

        let found = store.get_chunks_by_ids(&["known_id".into()]).await.unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].chunk_id, "known_id");
        assert_eq!(found[0].identifier, "test_func");

        // Unknown ID returns empty
        let not_found = store.get_chunks_by_ids(&["unknown".into()]).await.unwrap();
        assert_eq!(not_found.len(), 0);
    }
}
