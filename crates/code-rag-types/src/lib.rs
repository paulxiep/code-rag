use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// Generate SHA256 hash of content.
/// Normalizes CRLF → LF before hashing for cross-OS consistency.
pub fn content_hash(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.replace("\r\n", "\n").as_bytes());
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
    fn test_content_hash_crlf_normalization() {
        let hash_lf = content_hash("line1\nline2\n");
        let hash_crlf = content_hash("line1\r\nline2\r\n");
        assert_eq!(hash_lf, hash_crlf);
    }
}
