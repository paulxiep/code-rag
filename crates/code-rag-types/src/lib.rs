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
    fn test_content_hash_crlf_normalization() {
        let hash_lf = content_hash("line1\nline2\n");
        let hash_crlf = content_hash("line1\r\nline2\r\n");
        assert_eq!(hash_lf, hash_crlf);
    }
}
