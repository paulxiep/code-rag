//! Export all chunks with embeddings from LanceDB to JSON for static deployment.

use arrow_array::{Array, Float32Array, RecordBatch, StringArray, UInt64Array};
use code_rag_store::build_searchable_text;
use code_rag_types::{CodeChunk, CrateChunk, ModuleDocChunk, ReadmeChunk};
use futures::TryStreamExt;
use lancedb::query::ExecutableQuery;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tracing::info;

/// Pre-computed IDF table for browser-side BM25.
#[derive(Serialize)]
pub struct IdfTable {
    pub num_docs: usize,
    pub doc_frequencies: HashMap<String, usize>,
}

impl IdfTable {
    /// Build from an iterator of text content.
    /// Tokenizes identically to server-side `simple` tokenizer:
    /// split on non-alphanumeric boundaries, lowercase.
    #[allow(dead_code)]
    pub fn build(texts: impl Iterator<Item = impl AsRef<str>>) -> Self {
        let mut doc_frequencies: HashMap<String, usize> = HashMap::new();
        let mut num_docs = 0;
        for text in texts {
            num_docs += 1;
            let mut seen = HashSet::new();
            for token in tokenize(text.as_ref()) {
                if seen.insert(token.clone()) {
                    *doc_frequencies.entry(token).or_default() += 1;
                }
            }
        }
        Self {
            num_docs,
            doc_frequencies,
        }
    }
}

/// Tokenize identically to server-side `simple` tokenizer: split on non-alphanumeric, lowercase.
#[allow(dead_code)]
fn tokenize(text: &str) -> impl Iterator<Item = String> + '_ {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect::<Vec<_>>()
        .into_iter()
}

/// Matches the ChunkIndex format expected by code-rag-ui standalone mode.
#[derive(Serialize)]
pub struct ExportIndex {
    pub code_chunks: Vec<EmbeddedChunk<CodeChunk>>,
    pub readme_chunks: Vec<EmbeddedChunk<ReadmeChunk>>,
    pub crate_chunks: Vec<EmbeddedChunk<CrateChunk>>,
    pub module_doc_chunks: Vec<EmbeddedChunk<ModuleDocChunk>>,
    pub intent_prototypes: HashMap<String, Vec<Vec<f32>>>,
    pub projects: Vec<String>,
    /// IDF tables for browser-side BM25 (B2).
    /// None until hybrid search is enabled (post-B3 searchable_text column).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code_idf: Option<IdfTable>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub readme_idf: Option<IdfTable>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crate_idf: Option<IdfTable>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module_doc_idf: Option<IdfTable>,
}

#[derive(Serialize)]
pub struct EmbeddedChunk<T: Serialize> {
    #[serde(flatten)]
    pub chunk: T,
    pub embedding: Vec<f32>,
    /// B5: signature-text embedding. Only populated for code chunks that have
    /// a signature; all other chunk types and signature-less code chunks
    /// serialize with this field absent.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub signature_embedding: Option<Vec<f32>>,
}

pub async fn run_export(db_path: &str, output_path: &str) -> anyhow::Result<()> {
    let conn = lancedb::connect(db_path).execute().await?;

    let code_chunks = export_code_chunks(&conn).await?;
    info!("Exported {} code chunks", code_chunks.len());

    let readme_chunks = export_readme_chunks(&conn).await?;
    info!("Exported {} readme chunks", readme_chunks.len());

    let crate_chunks = export_crate_chunks(&conn).await?;
    info!("Exported {} crate chunks", crate_chunks.len());

    let module_doc_chunks = export_module_doc_chunks(&conn).await?;
    info!("Exported {} module doc chunks", module_doc_chunks.len());

    // Collect unique project names
    let mut projects: Vec<String> = code_chunks
        .iter()
        .map(|c| c.chunk.project_name.clone())
        .chain(readme_chunks.iter().map(|c| c.chunk.project_name.clone()))
        .collect();
    projects.sort();
    projects.dedup();

    // Build intent prototype embeddings
    let intent_prototypes = build_intent_prototypes()?;
    info!(
        "Built intent prototypes for {} categories",
        intent_prototypes.len()
    );

    // B3: Populate IDF tables for browser-side BM25.
    // Code chunks use searchable_text (identifier + signature + docstring).
    // Other chunk types use their natural text columns.
    let code_idf = Some(IdfTable::build(code_chunks.iter().map(|ec| {
        build_searchable_text(
            &ec.chunk.identifier,
            ec.chunk.signature.as_deref(),
            ec.chunk.docstring.as_deref(),
        )
    })));
    let readme_idf = Some(IdfTable::build(
        readme_chunks.iter().map(|ec| ec.chunk.content.clone()),
    ));
    let crate_idf = Some(IdfTable::build(crate_chunks.iter().filter_map(|ec| {
        ec.chunk.description.clone()
    })));
    let module_doc_idf = Some(IdfTable::build(
        module_doc_chunks
            .iter()
            .map(|ec| ec.chunk.doc_content.clone()),
    ));

    let index = ExportIndex {
        code_chunks,
        readme_chunks,
        crate_chunks,
        module_doc_chunks,
        intent_prototypes,
        projects,
        code_idf,
        readme_idf,
        crate_idf,
        module_doc_idf,
    };

    if let Some(parent) = std::path::Path::new(output_path).parent() {
        std::fs::create_dir_all(parent)?;
    }

    let json = serde_json::to_string(&index)?;
    std::fs::write(output_path, json)?;

    Ok(())
}

fn build_intent_prototypes() -> anyhow::Result<HashMap<String, Vec<Vec<f32>>>> {
    use code_rag_store::Embedder;

    let mut embedder = Embedder::new()?;

    let intent_texts: &[(&str, &[&str])] = &[
        (
            "overview",
            &[
                "What is this project?",
                "Tell me about this codebase",
                "Give me an overview",
                "What does this do?",
                "Describe the purpose",
                "What is the architecture?",
                "What is the purpose of this module?",
                "What is the role of this component?",
                "What is this package?",
            ],
        ),
        (
            "implementation",
            &[
                "How does this function work?",
                "Show me the implementation",
                "How is this implemented?",
                "Walk me through the logic",
                "How is this function implemented?",
                "Walk through this code step by step",
                "What are the steps of this algorithm?",
            ],
        ),
        (
            "relationship",
            &[
                "What calls this function?",
                "How does A relate to B?",
                "What depends on this?",
                "Show me the call chain",
                "What uses this module?",
                "What formats does this support?",
                "How do errors propagate through the system?",
            ],
        ),
        (
            "comparison",
            &[
                "Compare A and B",
                "What are the differences between X and Y?",
                "How does A differ from B?",
                "A versus B",
                "Contrast these approaches",
                "What are the pros and cons?",
                "What is the difference between X and Y?",
                "How does X compare to Y?",
                "Differences between X and Y",
            ],
        ),
    ];

    let mut prototypes = HashMap::new();
    for (name, texts) in intent_texts {
        let embeddings = embedder.embed_batch(texts)?;
        prototypes.insert(name.to_string(), embeddings);
    }

    Ok(prototypes)
}

// --- Arrow helpers ---

fn str_col<'a>(batch: &'a RecordBatch, name: &str) -> anyhow::Result<&'a StringArray> {
    batch
        .column_by_name(name)
        .and_then(|c: &Arc<dyn Array>| c.as_any().downcast_ref::<StringArray>())
        .ok_or_else(|| anyhow::anyhow!("missing column: {name}"))
}

fn opt_str_col<'a>(batch: &'a RecordBatch, name: &str) -> Option<&'a StringArray> {
    batch
        .column_by_name(name)
        .and_then(|c: &Arc<dyn Array>| c.as_any().downcast_ref::<StringArray>())
}

fn u64_col<'a>(batch: &'a RecordBatch, name: &str) -> anyhow::Result<&'a UInt64Array> {
    batch
        .column_by_name(name)
        .and_then(|c: &Arc<dyn Array>| c.as_any().downcast_ref::<UInt64Array>())
        .ok_or_else(|| anyhow::anyhow!("missing column: {name}"))
}

fn get_embedding(batch: &RecordBatch, row: usize) -> Vec<f32> {
    get_vector_column(batch, row, "vector").unwrap_or_default()
}

/// Read a fixed-size-list Float32 column by name at `row`.
/// Returns None when the column is absent OR the row is null.
fn get_vector_column(batch: &RecordBatch, row: usize, name: &str) -> Option<Vec<f32>> {
    let col = batch.column_by_name(name)?;
    let list = col
        .as_any()
        .downcast_ref::<arrow_array::FixedSizeListArray>()?;
    if list.is_null(row) {
        return None;
    }
    let value: Arc<dyn Array> = list.value(row);
    value
        .as_any()
        .downcast_ref::<Float32Array>()
        .map(|values| values.values().to_vec())
}

fn opt_str(arr: Option<&StringArray>, i: usize) -> Option<String> {
    arr.filter(|a: &&StringArray| !a.is_null(i))
        .map(|a: &StringArray| a.value(i).to_string())
}

async fn query_all(
    conn: &lancedb::Connection,
    table_name: &str,
) -> anyhow::Result<Vec<RecordBatch>> {
    let table: lancedb::Table = conn.open_table(table_name).execute().await?;
    let batches: Vec<RecordBatch> = table.query().execute().await?.try_collect().await?;
    Ok(batches)
}

async fn export_code_chunks(
    conn: &lancedb::Connection,
) -> anyhow::Result<Vec<EmbeddedChunk<CodeChunk>>> {
    let batches = query_all(conn, "code_chunks").await?;
    let mut result = Vec::new();

    for batch in &batches {
        let file_paths = str_col(batch, "file_path")?;
        let languages = str_col(batch, "language")?;
        let identifiers = str_col(batch, "identifier")?;
        let node_types = str_col(batch, "node_type")?;
        let code_contents = str_col(batch, "code_content")?;
        let chunk_ids = str_col(batch, "chunk_id")?;
        let content_hashes = str_col(batch, "content_hash")?;
        let model_versions = str_col(batch, "embedding_model_version")?;
        let project_names = str_col(batch, "project_name")?;
        let start_lines = u64_col(batch, "start_line")?;
        let docstrings = opt_str_col(batch, "docstring");
        let signatures = opt_str_col(batch, "signature");

        for i in 0..batch.num_rows() {
            let chunk = CodeChunk {
                file_path: file_paths.value(i).to_string(),
                language: languages.value(i).to_string(),
                identifier: identifiers.value(i).to_string(),
                node_type: node_types.value(i).to_string(),
                code_content: code_contents.value(i).to_string(),
                start_line: start_lines.value(i) as usize,
                project_name: project_names.value(i).to_string(),
                docstring: opt_str(docstrings, i),
                signature: opt_str(signatures, i),
                chunk_id: chunk_ids.value(i).to_string(),
                content_hash: content_hashes.value(i).to_string(),
                embedding_model_version: model_versions.value(i).to_string(),
            };
            result.push(EmbeddedChunk {
                chunk,
                embedding: get_embedding(batch, i),
                signature_embedding: get_vector_column(batch, i, "signature_vector"),
            });
        }
    }
    Ok(result)
}

async fn export_readme_chunks(
    conn: &lancedb::Connection,
) -> anyhow::Result<Vec<EmbeddedChunk<ReadmeChunk>>> {
    let batches = query_all(conn, "readme_chunks").await?;
    let mut result = Vec::new();

    for batch in &batches {
        let file_paths = str_col(batch, "file_path")?;
        let project_names = str_col(batch, "project_name")?;
        let contents = str_col(batch, "content")?;
        let chunk_ids = str_col(batch, "chunk_id")?;
        let content_hashes = str_col(batch, "content_hash")?;
        let model_versions = str_col(batch, "embedding_model_version")?;

        for i in 0..batch.num_rows() {
            let chunk = ReadmeChunk {
                file_path: file_paths.value(i).to_string(),
                project_name: project_names.value(i).to_string(),
                content: contents.value(i).to_string(),
                chunk_id: chunk_ids.value(i).to_string(),
                content_hash: content_hashes.value(i).to_string(),
                embedding_model_version: model_versions.value(i).to_string(),
            };
            result.push(EmbeddedChunk {
                chunk,
                embedding: get_embedding(batch, i),
                signature_embedding: None,
            });
        }
    }
    Ok(result)
}

async fn export_crate_chunks(
    conn: &lancedb::Connection,
) -> anyhow::Result<Vec<EmbeddedChunk<CrateChunk>>> {
    let batches = match query_all(conn, "crate_chunks").await {
        Ok(b) => b,
        Err(_) => return Ok(Vec::new()),
    };
    let mut result = Vec::new();

    for batch in &batches {
        let crate_names = str_col(batch, "crate_name")?;
        let crate_paths = str_col(batch, "crate_path")?;
        let project_names = str_col(batch, "project_name")?;
        let chunk_ids = str_col(batch, "chunk_id")?;
        let content_hashes = str_col(batch, "content_hash")?;
        let model_versions = str_col(batch, "embedding_model_version")?;
        let descriptions = opt_str_col(batch, "description");
        let deps_col = opt_str_col(batch, "dependencies");

        for i in 0..batch.num_rows() {
            let deps = opt_str(deps_col, i)
                .map(|s| {
                    serde_json::from_str::<Vec<String>>(&s)
                        .unwrap_or_else(|_| s.split(',').map(|d| d.trim().to_string()).collect())
                })
                .unwrap_or_default();

            let chunk = CrateChunk {
                crate_name: crate_names.value(i).to_string(),
                crate_path: crate_paths.value(i).to_string(),
                description: opt_str(descriptions, i),
                dependencies: deps,
                project_name: project_names.value(i).to_string(),
                chunk_id: chunk_ids.value(i).to_string(),
                content_hash: content_hashes.value(i).to_string(),
                embedding_model_version: model_versions.value(i).to_string(),
            };
            result.push(EmbeddedChunk {
                chunk,
                embedding: get_embedding(batch, i),
                signature_embedding: None,
            });
        }
    }
    Ok(result)
}

async fn export_module_doc_chunks(
    conn: &lancedb::Connection,
) -> anyhow::Result<Vec<EmbeddedChunk<ModuleDocChunk>>> {
    let batches = match query_all(conn, "module_doc_chunks").await {
        Ok(b) => b,
        Err(_) => return Ok(Vec::new()),
    };
    let mut result = Vec::new();

    for batch in &batches {
        let file_paths = str_col(batch, "file_path")?;
        let module_names = str_col(batch, "module_name")?;
        let doc_contents = str_col(batch, "doc_content")?;
        let project_names = str_col(batch, "project_name")?;
        let chunk_ids = str_col(batch, "chunk_id")?;
        let content_hashes = str_col(batch, "content_hash")?;
        let model_versions = str_col(batch, "embedding_model_version")?;

        for i in 0..batch.num_rows() {
            let chunk = ModuleDocChunk {
                file_path: file_paths.value(i).to_string(),
                module_name: module_names.value(i).to_string(),
                doc_content: doc_contents.value(i).to_string(),
                project_name: project_names.value(i).to_string(),
                chunk_id: chunk_ids.value(i).to_string(),
                content_hash: content_hashes.value(i).to_string(),
                embedding_model_version: model_versions.value(i).to_string(),
            };
            result.push(EmbeddedChunk {
                chunk,
                embedding: get_embedding(batch, i),
                signature_embedding: None,
            });
        }
    }
    Ok(result)
}
