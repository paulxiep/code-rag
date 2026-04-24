use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// Bump when the chunk-derivation pipeline changes in a way that affects
/// columns persisted alongside CodeChunk (currently `searchable_text` and
/// `signature_vector`) but not the raw `code_content`. The reconcile
/// mechanism only sees a chunk as "changed" when its `content_hash` shifts;
/// mixing this constant into the per-file hash forces re-ingestion when the
/// derivation logic changes even if source bytes are unchanged.
///
/// Bump on:
/// - `build_searchable_text` formula change (B3/C4 territory)
/// - `signature_vector` embedding strategy change (B5)
/// - any new column derived from CodeChunk fields at write time
pub const DERIVATION_VERSION: &str = "v1";

/// Generate SHA256 hash of content.
/// Normalizes CRLF → LF before hashing for cross-OS consistency.
pub fn content_hash(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.replace("\r\n", "\n").as_bytes());
    format!("{:x}", hasher.finalize())
}

/// File-level hash for CodeChunks: source content + derivation-version +
/// per-chunk signature/docstring fingerprints. Reconcile compares this hash
/// against the value stored in LanceDB; mismatch → file's chunks get
/// deleted and re-embedded. Including `signature` and `docstring` here
/// catches parser changes that don't touch source bytes; the
/// `DERIVATION_VERSION` prefix catches downstream-formula changes that
/// don't touch the parser either.
pub fn code_chunk_file_hash<'a, I>(file_content: &str, chunks: I) -> String
where
    I: IntoIterator<Item = (Option<&'a str>, Option<&'a str>)>,
{
    let mut hasher = Sha256::new();
    hasher.update(b"derivation:");
    hasher.update(DERIVATION_VERSION.as_bytes());
    hasher.update(b":src:");
    hasher.update(file_content.replace("\r\n", "\n").as_bytes());
    for (signature, docstring) in chunks {
        hasher.update(b":sig:");
        hasher.update(signature.unwrap_or("").as_bytes());
        hasher.update(b":doc:");
        hasher.update(docstring.unwrap_or("").as_bytes());
    }
    format!("{:x}", hasher.finalize())
}

/// Generate new UUID v4
pub fn new_chunk_id() -> String {
    Uuid::new_v4().to_string()
}

/// Deterministic chunk ID from file path + content.
/// Same function in same file = same ID across re-indexing runs.
/// Stable foreign key for Track C call graph edges.
pub fn deterministic_chunk_id(file_path: &str, content: &str) -> String {
    content_hash(&format!("chunk:{}:{}", file_path, content))
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CodeChunk {
    pub file_path: String,
    pub language: String,     // "rust" or "python"
    pub identifier: String,   // Function/Class name
    pub node_type: String,    // "function_definition", "class_definition"
    pub code_content: String, // The actual snippet for the LLM
    pub start_line: usize,
    pub project_name: String,      // e.g., "7_wonders", "catan"
    pub docstring: Option<String>, // Extracted documentation

    /// Extracted declaration signature (B3).
    /// Functions: "pub async fn retrieve(query: &str) -> Result<Vec<CodeChunk>>"
    /// Structs/enums/traits: "pub struct VectorStore", "pub trait Foo: Send + Sync"
    /// None for macro_definition and if extraction fails.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,

    /// Deterministic ID: hash(file_path, content). Stable across re-indexing for unchanged chunks.
    /// Foreign key for Track C call graph edges.
    pub chunk_id: String,
    /// SHA256 of source file content for file-level change detection
    pub content_hash: String,
    /// Embedding model identifier, e.g., "BGESmallENV15_384"
    pub embedding_model_version: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ReadmeChunk {
    pub file_path: String,
    pub project_name: String,
    pub content: String,

    /// Deterministic ID: hash(file_path, content). Stable across re-indexing for unchanged chunks.
    pub chunk_id: String,
    /// SHA256 of file content for change detection
    pub content_hash: String,
    /// Embedding model identifier
    pub embedding_model_version: String,
}

/// Represents a Rust crate extracted from Cargo.toml
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CrateChunk {
    pub crate_name: String,
    pub crate_path: String,          // Path to the crate directory
    pub description: Option<String>, // From [package].description
    pub dependencies: Vec<String>,   // Workspace/local dependencies
    pub project_name: String,        // Parent project (e.g., "quant-trading-gym")

    /// Deterministic ID: hash(file_path, content). Stable across re-indexing for unchanged chunks.
    pub chunk_id: String,
    /// SHA256 of serialized metadata for change detection (crate_name:description:deps)
    pub content_hash: String,
    /// Embedding model identifier
    pub embedding_model_version: String,
}

/// A directed edge in the call graph: caller → callee.
/// Stored once per edge. Queried both directions via filter predicates.
/// No embedding vector — pure scalar table in LanceDB.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CallEdge {
    /// Deterministic: content_hash("edge:{caller_chunk_id}:{callee_chunk_id}")
    pub edge_id: String,
    /// FK to CodeChunk.chunk_id
    pub caller_chunk_id: String,
    /// FK to CodeChunk.chunk_id
    pub callee_chunk_id: String,
    /// Human-readable caller function name
    pub caller_identifier: String,
    /// Human-readable callee function name
    pub callee_identifier: String,
    /// Normalized path of caller's file
    pub caller_file: String,
    /// Normalized path of callee's file
    pub callee_file: String,
    pub project_name: String,
    /// Resolution confidence: 1=same_file, 2=import_based, 3=unique_global
    pub resolution_tier: u8,
}

/// Compact call edge for JSON export (WASM standalone demo).
/// Browser looks up file/identifier from existing chunk data.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ExportEdge {
    pub caller: String,
    pub callee: String,
    pub tier: u8,
}

/// Represents module-level documentation (//! comments at top of lib.rs)
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ModuleDocChunk {
    pub file_path: String,
    pub module_name: String, // Derived from file/crate name
    pub doc_content: String, // The //! doc comments
    pub project_name: String,

    /// Deterministic ID: hash(file_path, content). Stable across re-indexing for unchanged chunks.
    pub chunk_id: String,
    /// SHA256 of source file content for change detection
    pub content_hash: String,
    /// Embedding model identifier
    pub embedding_model_version: String,
}

/// A2: Folder-level summary chunk. One per directory in the portfolio.
///
/// Deterministic, template-rendered summary of a folder's contents (no LLM):
/// file count, distinct languages, public types, public functions, direct
/// subfolders. Answers directory-level Overview queries like "What does the
/// engine/ folder do?" which vector search over function chunks cannot.
///
/// `summary_text` is the exact string that is embedded server-side and
/// BM25-scored browser-side — persisted to avoid re-render drift.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FolderChunk {
    /// Project-prefixed, forward-slash folder path relative to the portfolio
    /// root. Matches CodeChunk.file_path convention.
    /// Example: "code-rag/crates/code-rag-engine/src".
    pub folder_path: String,
    /// Project this folder belongs to (sibling subdir of the ingest root).
    pub project_name: String,
    /// Source files directly in this folder (non-recursive).
    pub file_count: usize,
    /// Distinct language names for source files directly in this folder.
    pub languages: Vec<String>,
    /// Public types (structs/enums/traits/classes/interfaces) in this folder.
    /// Alphabetical, deduped, capped at 12.
    pub key_types: Vec<String>,
    /// Public functions in this folder. Alphabetical, deduped, capped at 12.
    pub key_functions: Vec<String>,
    /// Direct child directory names (one level).
    pub subfolders: Vec<String>,
    /// Pre-rendered template string — same bytes embedded and BM25-scored.
    /// First line: `Folder: {folder_path} (module: {basename})` — the
    /// `module:` synonym lets BM25 hit queries phrased "X module" without
    /// query-time expansion.
    pub summary_text: String,

    /// Deterministic ID: hash(folder_path, summary_text).
    pub chunk_id: String,
    /// SHA256 of the canonicalized metadata tuple (enables skip-unchanged).
    pub content_hash: String,
    /// Embedding model identifier
    pub embedding_model_version: String,
}

/// A4: File-level summary chunk. One per source file that produced CodeChunks.
///
/// Sits between CodeChunks (one per function/type) and FolderChunks
/// (one per directory) in the collapsed-tree hierarchy. Answers file-level
/// queries like "what does retriever.rs do?" or "which files depend on
/// fastembed?" — questions that either fragment across function chunks or
/// aren't answerable by folder summaries.
///
/// `summary_text` is the pre-rendered template embedded server-side and
/// BM25-scored browser-side — persisted to avoid re-render drift.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FileChunk {
    /// Project-prefixed, forward-slash file path. Matches CodeChunk.file_path.
    /// Example: "code-rag/crates/code-rag-engine/src/retriever.rs".
    pub file_path: String,
    /// Project this file belongs to (sibling subdir of the ingest root).
    pub project_name: String,
    /// "rust" | "python" | "typescript" | etc. Same vocab as CodeChunk.language.
    pub language: String,
    /// Public items (types + functions) defined in this file.
    /// Alphabetical, deduped, capped at 16.
    pub exports: Vec<String>,
    /// External / cross-file imports used by this file, formatted as
    /// `"{imported_name} from {source_path}"` (source of the strings is
    /// LanguageHandler::extract_file_imports, the C1 method).
    /// Alphabetical, deduped, capped at 16.
    pub imports: Vec<String>,
    /// Inferred one-sentence purpose, ≤140 chars. Source order:
    ///   1. First line of the module-level doc chunk for this file, if any.
    ///   2. First docstring in the file (smallest start_line), if any.
    ///   3. Filename-derived fallback: "This file defines {basename}."
    pub purpose: Option<String>,
    /// Pre-rendered template — same bytes embedded and BM25-scored.
    /// First line: `File: {file_path} (module: {basename}, {language})`.
    pub summary_text: String,

    /// Deterministic ID: hash("file:{file_path}", summary_text).
    pub chunk_id: String,
    /// SHA256 of the canonicalized metadata tuple (enables skip-unchanged).
    pub content_hash: String,
    /// Embedding model identifier
    pub embedding_model_version: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_content_hash_deterministic() {
        let content = "fn foo() {}";
        let hash1 = content_hash(content);
        let hash2 = content_hash(content);
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_content_hash_different() {
        let hash1 = content_hash("fn foo() {}");
        let hash2 = content_hash("fn bar() {}");
        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_content_hash_format() {
        let hash = content_hash("test");
        // SHA256 produces 64 hex characters
        assert_eq!(hash.len(), 64);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_chunk_id_unique() {
        let id1 = new_chunk_id();
        let id2 = new_chunk_id();
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_chunk_id_format() {
        let id = new_chunk_id();
        // UUID v4 format: 8-4-4-4-12 = 36 characters
        assert_eq!(id.len(), 36);
        assert!(id.chars().filter(|&c| c == '-').count() == 4);
    }

    #[test]
    fn test_deterministic_chunk_id_stable() {
        let id1 = deterministic_chunk_id("src/lib.rs", "fn foo() {}");
        let id2 = deterministic_chunk_id("src/lib.rs", "fn foo() {}");
        assert_eq!(id1, id2);
    }

    #[test]
    fn test_deterministic_chunk_id_different_content() {
        let id1 = deterministic_chunk_id("src/lib.rs", "fn foo() {}");
        let id2 = deterministic_chunk_id("src/lib.rs", "fn bar() {}");
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_deterministic_chunk_id_different_path() {
        let id1 = deterministic_chunk_id("src/a.rs", "fn foo() {}");
        let id2 = deterministic_chunk_id("src/b.rs", "fn foo() {}");
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_call_edge_deterministic_edge_id() {
        let edge_id1 = content_hash(&format!("edge:{}:{}", "chunk_a", "chunk_b"));
        let edge_id2 = content_hash(&format!("edge:{}:{}", "chunk_a", "chunk_b"));
        assert_eq!(edge_id1, edge_id2);
    }

    #[test]
    fn test_call_edge_different_direction_different_id() {
        let id_ab = content_hash(&format!("edge:{}:{}", "chunk_a", "chunk_b"));
        let id_ba = content_hash(&format!("edge:{}:{}", "chunk_b", "chunk_a"));
        assert_ne!(id_ab, id_ba);
    }

    #[test]
    fn test_export_edge_clone() {
        let edge = ExportEdge {
            caller: "chunk_a".into(),
            callee: "chunk_b".into(),
            tier: 1,
        };
        let cloned = edge.clone();
        assert_eq!(cloned.caller, "chunk_a");
        assert_eq!(cloned.callee, "chunk_b");
        assert_eq!(cloned.tier, 1);
    }

    #[test]
    fn test_code_chunk_file_hash_invariant_to_chunk_iteration_order_within_file() {
        // Stable as long as caller iterates chunks in the same order.
        let h1 = code_chunk_file_hash(
            "fn a() {}\nfn b() {}",
            vec![(Some("fn a()"), None), (Some("fn b()"), Some("doc b"))],
        );
        let h2 = code_chunk_file_hash(
            "fn a() {}\nfn b() {}",
            vec![(Some("fn a()"), None), (Some("fn b()"), Some("doc b"))],
        );
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_code_chunk_file_hash_changes_when_signature_changes() {
        let src = "fn foo(x: u32) -> u32 { x }";
        let h_old = code_chunk_file_hash(src, vec![(Some("fn foo(x: u32) -> u32"), None)]);
        let h_new = code_chunk_file_hash(src, vec![(Some("pub fn foo(x: u32) -> u32"), None)]);
        // Same source bytes, different extracted signature — must invalidate
        // so reconcile re-fires and the persisted searchable_text /
        // signature_vector get refreshed.
        assert_ne!(h_old, h_new);
    }

    #[test]
    fn test_code_chunk_file_hash_changes_when_docstring_changes() {
        let src = "fn foo() {}";
        let h_old = code_chunk_file_hash(src, vec![(Some("fn foo()"), Some("old doc"))]);
        let h_new = code_chunk_file_hash(src, vec![(Some("fn foo()"), Some("new doc"))]);
        assert_ne!(h_old, h_new);
    }

    #[test]
    fn test_code_chunk_file_hash_changes_when_source_changes() {
        let h_old = code_chunk_file_hash("fn foo() {}", vec![(Some("fn foo()"), None)]);
        let h_new = code_chunk_file_hash("fn foo() { 1 }", vec![(Some("fn foo()"), None)]);
        assert_ne!(h_old, h_new);
    }

    #[test]
    fn test_code_chunk_file_hash_crlf_normalized() {
        let h_lf = code_chunk_file_hash("fn a() {}\nfn b() {}", vec![]);
        let h_crlf = code_chunk_file_hash("fn a() {}\r\nfn b() {}", vec![]);
        assert_eq!(h_lf, h_crlf);
    }

    #[test]
    fn test_code_chunk_file_hash_differs_from_plain_content_hash() {
        // The DERIVATION_VERSION prefix means the new function never
        // collides with the old plain content_hash for the same input —
        // upgrading to v1 invalidates every existing chunk in the DB,
        // forcing a clean re-embed (intentional one-time wipe-equivalent).
        let src = "fn foo() {}";
        assert_ne!(content_hash(src), code_chunk_file_hash(src, vec![]));
    }

    #[test]
    fn test_content_hash_crlf_normalization() {
        let hash_lf = content_hash("line1\nline2\n");
        let hash_crlf = content_hash("line1\r\nline2\r\n");
        assert_eq!(hash_lf, hash_crlf);
    }

    #[test]
    fn test_file_chunk_id_distinct_from_folder_chunk_id() {
        let path = "code-rag/crates/code-rag-engine/src/retriever.rs";
        let summary = "File: retriever.rs\n...";
        let file_id = deterministic_chunk_id(&format!("file:{}", path), summary);
        let folder_id = deterministic_chunk_id(&format!("folder:{}", path), summary);
        let raw_id = deterministic_chunk_id(path, summary);
        assert_ne!(file_id, folder_id);
        assert_ne!(file_id, raw_id);
        assert_ne!(folder_id, raw_id);
    }
}
