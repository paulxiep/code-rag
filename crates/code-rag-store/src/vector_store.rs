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

use code_rag_types::{CodeChunk, CrateChunk, ModuleDocChunk, ReadmeChunk};

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

/// LanceDB-backed vector store for code and readme chunks.
pub struct VectorStore {
    conn: Connection,
    dimension: usize,
}

impl VectorStore {
    /// Connect to LanceDB at the given path (creates if not exists).
    pub async fn new(db_path: &str, embedding_dimension: usize) -> Result<Self, StoreError> {
        // Ensure parent directory exists (important for Docker bind mounts)
        if let Some(parent) = std::path::Path::new(db_path).parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let conn = connect(db_path).execute().await?;
        Ok(Self {
            conn,
            dimension: embedding_dimension,
        })
    }

    pub fn dimension(&self) -> usize {
        self.dimension
    }

    // ========================================================================
    // Write operations (used by code-raptor)
    // ========================================================================

    /// Insert code chunks with their embeddings. Creates table if needed.
    pub async fn upsert_code_chunks(
        &self,
        chunks: &[CodeChunk],
        embeddings: Vec<Vec<f32>>,
    ) -> Result<usize, StoreError> {
        if chunks.is_empty() {
            return Ok(0);
        }

        let batch = code_chunks_to_batch(chunks, embeddings, self.dimension)?;
        let count = batch.num_rows();

        self.upsert_batch(CODE_TABLE, batch).await?;
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

        self.upsert_batch(README_TABLE, batch).await?;
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

        self.upsert_batch(CRATE_TABLE, batch).await?;
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

        self.upsert_batch(MODULE_DOC_TABLE, batch).await?;
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

    /// Search code chunks by vector similarity. Returns (chunk, distance) pairs.
    pub async fn search_code(
        &self,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<(CodeChunk, f32)>, StoreError> {
        let table = self.get_table(CODE_TABLE).await?;

        let results = table
            .vector_search(query_embedding.to_vec())?
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

    async fn upsert_batch(&self, table_name: &str, batch: RecordBatch) -> Result<(), StoreError> {
        let schema = batch.schema();

        // Try to open existing table, create if not exists
        match self.conn.open_table(table_name).execute().await {
            Ok(table) => {
                let batches = RecordBatchIterator::new(vec![Ok(batch)], schema);
                table.add(batches).execute().await?;
            }
            Err(_) => {
                let batches = RecordBatchIterator::new(vec![Ok(batch)], schema);
                self.conn
                    .create_table(table_name, batches)
                    .execute()
                    .await?;
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
}

// ============================================================================
// Arrow conversion functions (pure, no side effects)
// ============================================================================

fn code_chunks_to_batch(
    chunks: &[CodeChunk],
    embeddings: Vec<Vec<f32>>,
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

// --- B3: Searchable text construction ---

/// Build searchable_text from high-signal fields only.
/// Excludes code body and calls — these dilute BM25 signal.
///
/// Two BM25 optimizations:
/// 1. Identifier repeated 2x — simulates field-level boosting (LanceDB single-column FTS)
/// 2. camelCase/PascalCase split into component words alongside original
pub fn build_searchable_text(
    identifier: &str,
    signature: Option<&str>,
    docstring: Option<&str>,
) -> String {
    let mut parts = Vec::new();

    // Identifier with boost (2x) + camelCase split
    let split = split_camel_case(identifier);
    if split != identifier.to_lowercase() {
        parts.push(format!("{} {} {}", identifier, identifier, split));
    } else {
        parts.push(format!("{} {}", identifier, identifier));
    }

    if let Some(sig) = signature {
        parts.push(sig.to_string());
    }
    if let Some(doc) = docstring {
        if !doc.is_empty() {
            parts.push(doc.to_string());
        }
    }
    parts.join("\n")
}

/// Split camelCase/PascalCase into lowercase words.
/// "VectorStore" → "vector store"
/// "parseHTTPResponse" → "parse http response"
/// "snake_case" → "snake_case" (unchanged, already split by tokenizer)
pub fn split_camel_case(s: &str) -> String {
    let mut words = Vec::new();
    let mut current = String::new();
    let chars: Vec<char> = s.chars().collect();

    for i in 0..chars.len() {
        let c = chars[i];
        if c.is_uppercase() && !current.is_empty() {
            let prev_upper = i > 0 && chars[i - 1].is_uppercase();
            let next_lower = i + 1 < chars.len() && chars[i + 1].is_lowercase();
            if !prev_upper || next_lower {
                words.push(current.to_lowercase());
                current = String::new();
            }
        }
        current.push(c);
    }
    if !current.is_empty() {
        words.push(current.to_lowercase());
    }
    words.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

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

        let batch = code_chunks_to_batch(&chunks, embeddings, 384).unwrap();

        assert_eq!(batch.num_rows(), 1);
        assert_eq!(batch.num_columns(), 14); // 13 fields + vector (11 original + signature + searchable_text)
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

        let batch = code_chunks_to_batch(&chunks, embeddings, 384).unwrap();

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

        let count = store.upsert_code_chunks(&chunks, embeddings).await.unwrap();
        assert_eq!(count, 1);

        let results = store.search_code(&fake_embedding(384), 10).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0.identifier, "test_func");
    }
}
