pub mod file;
pub mod folder;
pub mod language;
pub mod languages;
pub mod parser;
pub mod reconcile;

pub use language::{ImportInfo, LanguageHandler};
pub use languages::{handler_by_name, handler_for_path, supported_extensions};
pub use reconcile::{
    DeletionsByTable, ExistingFileIndex, IngestionStats, ReconcileResult, reconcile,
};

use self::parser::{CodeAnalyzer, parse_cargo_toml};
use code_rag_types::{
    CodeChunk, CrateChunk, ModuleDocChunk, ReadmeChunk, code_chunk_file_hash, content_hash,
    deterministic_chunk_id,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tracing::warn;
use walkdir::{DirEntry, WalkDir};

/// Default embedding model version (matches embedder.rs)
pub const DEFAULT_EMBEDDING_MODEL: &str = "BGESmallENV15_384";

/// Extract project name from file path based on directory structure.
/// Returns the top-level directory name relative to the repo root,
/// or None for root-level files.
fn extract_project_name(path: &Path, repo_root: &Path) -> Option<String> {
    let relative = path.strip_prefix(repo_root).ok()?;
    let mut components = relative.components();
    let first = components.next()?;

    // If this is the only component, it's a root-level file
    components.next()?;

    first.as_os_str().to_str().map(|s| s.to_string())
}

/// Resolve project name — never returns None.
/// Priority: CLI override > subdirectory name > repo directory name > "unknown"
fn resolve_project_name(path: &Path, repo_root: &Path, cli_override: Option<&str>) -> String {
    if let Some(name) = cli_override {
        return name.to_string();
    }
    extract_project_name(path, repo_root).unwrap_or_else(|| {
        repo_root
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string()
    })
}

/// Normalize a path for storage: relative to repo_root, forward slashes
pub(crate) fn normalize_path(path: &Path, repo_root: &Path) -> String {
    let relative = path.strip_prefix(repo_root).unwrap_or(path);
    relative.to_string_lossy().replace('\\', "/")
}

/// Check if entry is a README file
fn is_readme(entry: &DirEntry) -> bool {
    entry
        .path()
        .file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.eq_ignore_ascii_case("readme.md"))
        .unwrap_or(false)
}

/// Check if entry is a Cargo.toml file
fn is_cargo_toml(entry: &DirEntry) -> bool {
    entry
        .path()
        .file_name()
        .and_then(|n| n.to_str())
        .map(|n| n == "Cargo.toml")
        .unwrap_or(false)
}

/// Check if entry is a lib.rs file (for module doc extraction)
fn is_lib_rs(entry: &DirEntry) -> bool {
    entry
        .path()
        .file_name()
        .and_then(|n| n.to_str())
        .map(|n| n == "lib.rs")
        .unwrap_or(false)
}

/// Process a single README file
fn process_readme(
    entry: &DirEntry,
    repo_root: &Path,
    project_name_override: Option<&str>,
) -> Option<ReadmeChunk> {
    let content = std::fs::read_to_string(entry.path()).ok()?;
    let project_name = resolve_project_name(entry.path(), repo_root, project_name_override);

    let norm_path = normalize_path(entry.path(), repo_root);

    Some(ReadmeChunk {
        chunk_id: deterministic_chunk_id(&norm_path, &content),
        content_hash: content_hash(&content),
        file_path: norm_path,
        project_name,
        embedding_model_version: DEFAULT_EMBEDDING_MODEL.to_string(),
        content,
    })
}

/// Process a Cargo.toml file to extract crate metadata
fn process_cargo_toml(
    entry: &DirEntry,
    repo_root: &Path,
    project_name_override: Option<&str>,
) -> Option<CrateChunk> {
    let content = std::fs::read_to_string(entry.path()).ok()?;
    let (crate_name, description, dependencies) = parse_cargo_toml(&content)?;

    let crate_path = normalize_path(entry.path().parent()?, repo_root);
    let project_name = resolve_project_name(entry.path(), repo_root, project_name_override);

    // Hash all embedding-relevant fields for change detection
    let hash_content = format!(
        "{}:{}:{}",
        crate_name,
        description.as_deref().unwrap_or(""),
        dependencies.join(",")
    );

    Some(CrateChunk {
        chunk_id: deterministic_chunk_id(&crate_path, &hash_content),
        content_hash: content_hash(&hash_content),
        crate_name,
        crate_path,
        description,
        dependencies,
        project_name,
        embedding_model_version: DEFAULT_EMBEDDING_MODEL.to_string(),
    })
}

/// Process a lib.rs file to extract module-level documentation
fn process_module_docs(
    entry: &DirEntry,
    repo_root: &Path,
    analyzer: &mut CodeAnalyzer,
    project_name_override: Option<&str>,
) -> Option<ModuleDocChunk> {
    let content = std::fs::read_to_string(entry.path()).ok()?;
    let doc_content = analyzer.extract_module_docs(&content)?;

    // Derive module name from parent directory (usually the crate name)
    let module_name = entry
        .path()
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();

    let project_name = resolve_project_name(entry.path(), repo_root, project_name_override);

    let norm_path = normalize_path(entry.path(), repo_root);

    Some(ModuleDocChunk {
        chunk_id: deterministic_chunk_id(&norm_path, &doc_content),
        content_hash: content_hash(&content), // hash entire file for change detection
        file_path: norm_path,
        module_name,
        embedding_model_version: DEFAULT_EMBEDDING_MODEL.to_string(),
        doc_content,
        project_name,
    })
}

/// File-level imports keyed by file path; emitted by `process_code_file` so
/// the caller can fold many files into a single `ImportsMap`.
struct FileImports {
    path: String,
    imports: Vec<ImportInfo>,
}

/// Result of processing a single code file: extracted chunks, the call graph
/// keyed by chunk_id, and (optionally) the file's imports for later edge
/// resolution.
struct ProcessedCodeFile {
    chunks: Vec<CodeChunk>,
    calls: CallsMap,
    file_imports: Option<FileImports>,
}

/// Process a single code file.
fn process_code_file(
    entry: &DirEntry,
    repo_root: &Path,
    analyzer: &mut CodeAnalyzer,
    project_name_override: Option<&str>,
) -> ProcessedCodeFile {
    let empty = || ProcessedCodeFile {
        chunks: Vec::new(),
        calls: HashMap::new(),
        file_imports: None,
    };

    // Skip files with unsupported extensions before reading (avoids UTF-8 errors on binary files)
    let handler = match handler_for_path(entry.path()) {
        Some(h) => h,
        None => return empty(),
    };

    let content = match std::fs::read_to_string(entry.path()) {
        Ok(c) => c,
        Err(e) => {
            warn!(path = %entry.path().display(), error = %e, "Failed to read file");
            return empty();
        }
    };

    let pairs = analyzer.analyze_file(entry.path(), &content);

    if pairs.is_empty() {
        return empty();
    }

    let path_str = normalize_path(entry.path(), repo_root);
    let project_name = resolve_project_name(entry.path(), repo_root, project_name_override);
    // Mix per-chunk signature/docstring + DERIVATION_VERSION into the file
    // hash so reconcile re-fires when extraction logic or downstream
    // derivation (searchable_text, signature_vector) changes — not just
    // when source bytes change.
    let file_hash = code_chunk_file_hash(
        &content,
        pairs
            .iter()
            .map(|(c, _)| (c.signature.as_deref(), c.docstring.as_deref())),
    );

    // C1: Extract file-level imports for edge resolution
    let file_imports = {
        let mut parser = tree_sitter::Parser::new();
        let grammar = handler.grammar();
        if parser.set_language(&grammar).is_ok()
            && let Some(tree) = parser.parse(&content, None)
        {
            let imports =
                handler.extract_file_imports(&content, &tree.root_node(), content.as_bytes());
            if !imports.is_empty() {
                Some(FileImports {
                    path: path_str.clone(),
                    imports,
                })
            } else {
                None
            }
        } else {
            None
        }
    };

    // Enrich metadata, then split into chunks and calls map
    let enriched: Vec<_> = pairs
        .into_iter()
        .map(|(mut chunk, calls)| {
            chunk.file_path.clone_from(&path_str);
            chunk.project_name.clone_from(&project_name);
            chunk.content_hash.clone_from(&file_hash);
            chunk.chunk_id = deterministic_chunk_id(&path_str, &chunk.code_content);
            (chunk, calls)
        })
        .collect();

    let (chunks, call_entries): (Vec<_>, Vec<_>) = enriched
        .into_iter()
        .map(|(chunk, calls)| {
            let entry = (!calls.is_empty()).then(|| (chunk.chunk_id.clone(), calls));
            (chunk, entry)
        })
        .unzip();

    let calls_map: HashMap<String, Vec<String>> = call_entries.into_iter().flatten().collect();

    ProcessedCodeFile {
        chunks,
        calls: calls_map,
        file_imports,
    }
}

/// Directories to skip during ingestion
pub(crate) const IGNORED_DIRS: &[&str] = &[
    ".git",
    "target",
    "node_modules",
    "__pycache__",
    ".venv",
    "venv",
    ".idea",
    ".vscode",
    "dist",
    "build",
    "tests", // Rust integration tests, Python test dirs
    "test",  // Common test directory name
];

/// Check if path contains any ignored directory (relative to repo root)
pub(crate) fn should_skip(entry: &DirEntry, repo_root: &Path) -> bool {
    // Only check components within the repo, not the full system path
    let relative = match entry.path().strip_prefix(repo_root) {
        Ok(r) => r,
        Err(_) => return false,
    };

    // Directory-level: skip ignored dirs and hidden dirs
    if relative.components().any(|c| {
        c.as_os_str()
            .to_str()
            .map(|s| IGNORED_DIRS.contains(&s) || s.starts_with('.'))
            .unwrap_or(false)
    }) {
        return true;
    }

    // File-level: skip test files by naming convention
    if let Some(name) = entry.path().file_name().and_then(|n| n.to_str()) {
        let name_lower = name.to_lowercase();
        // Python: test_*.py, *_test.py
        if name_lower.ends_with(".py")
            && (name_lower.starts_with("test_") || name_lower.ends_with("_test.py"))
        {
            return true;
        }
        // TypeScript/JavaScript: *.test.ts, *.spec.ts, *.test.tsx, *.spec.tsx
        if name_lower.ends_with(".test.ts")
            || name_lower.ends_with(".spec.ts")
            || name_lower.ends_with(".test.tsx")
            || name_lower.ends_with(".spec.tsx")
            || name_lower.ends_with(".test.js")
            || name_lower.ends_with(".spec.js")
        {
            return true;
        }
    }

    false
}

/// Result of running ingestion on a repository
#[derive(Debug, Clone, Default)]
pub struct IngestionResult {
    pub code_chunks: Vec<CodeChunk>,
    pub readme_chunks: Vec<ReadmeChunk>,
    pub crate_chunks: Vec<CrateChunk>,
    pub module_doc_chunks: Vec<ModuleDocChunk>,
    /// A2: folder-level summaries. One per non-empty, non-vendor directory.
    pub folder_chunks: Vec<code_rag_types::FolderChunk>,
    /// A4: file-level summaries. One per source file that produced CodeChunks.
    pub file_chunks: Vec<code_rag_types::FileChunk>,
}

/// Type aliases for ingestion side-channels.
pub type CallsMap = HashMap<String, Vec<String>>;
pub type ImportsMap = HashMap<String, Vec<ImportInfo>>;

/// This orchestrates the flow from Disk -> Parser -> Data.
/// `project_name_override`: if Some, all chunks get this project name.
/// If None, project name is inferred from directory structure.
///
/// Returns `(IngestionResult, calls_map, imports_map)` where:
/// - `calls_map` maps `chunk_id → call identifiers` (for embedding enrichment)
/// - `imports_map` maps `file_path → imports` (for C1 edge resolution)
pub fn run_ingestion(
    repo_path: &str,
    project_name_override: Option<&str>,
) -> (IngestionResult, CallsMap, ImportsMap) {
    let repo_root = PathBuf::from(repo_path);
    let mut analyzer = CodeAnalyzer::new();

    let entries: Vec<_> = WalkDir::new(repo_path)
        .follow_links(false) // Don't follow symlinks to avoid cycles
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter(|e| !should_skip(e, &repo_root))
        .collect();

    let readme_chunks: Vec<ReadmeChunk> = entries
        .iter()
        .filter(|e| is_readme(e))
        .filter_map(|e| process_readme(e, &repo_root, project_name_override))
        .collect();

    let mut all_chunks: Vec<CodeChunk> = Vec::new();
    let mut all_calls: CallsMap = HashMap::new();
    let mut all_imports: ImportsMap = HashMap::new();

    for e in &entries {
        let processed = process_code_file(e, &repo_root, &mut analyzer, project_name_override);
        all_chunks.extend(processed.chunks);
        all_calls.extend(processed.calls);
        if let Some(fi) = processed.file_imports {
            all_imports.insert(fi.path, fi.imports);
        }
    }

    let crate_chunks: Vec<CrateChunk> = entries
        .iter()
        .filter(|e| is_cargo_toml(e))
        .filter_map(|e| process_cargo_toml(e, &repo_root, project_name_override))
        .collect();

    let module_doc_chunks: Vec<ModuleDocChunk> = entries
        .iter()
        .filter(|e| is_lib_rs(e))
        .filter_map(|e| process_module_docs(e, &repo_root, &mut analyzer, project_name_override))
        .collect();

    // A2: Produce folder-level summary chunks. Runs after code_chunks are
    // assembled so key_types/key_functions can be derived from the already-
    // parsed code chunks. Uses the shared `should_skip` blocklist — no
    // duplication with the main walk.
    let folder_chunks = folder::build_folder_chunks(repo_path, &all_chunks, project_name_override);

    // A4: Produce file-level summary chunks. Runs at the same seam as
    // folder (after code chunks are assembled) and consumes the imports_map
    // while it's still in scope — no need to thread it out.
    let file_chunks = file::build_file_chunks(
        &all_chunks,
        &module_doc_chunks,
        &all_imports,
        project_name_override,
    );

    (
        IngestionResult {
            code_chunks: all_chunks,
            readme_chunks,
            crate_chunks,
            module_doc_chunks,
            folder_chunks,
            file_chunks,
        },
        all_calls,
        all_imports,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_test_workspace() -> TempDir {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();

        // Create project1 with a Python file and README
        fs::create_dir(base.join("project1")).unwrap();
        fs::write(
            base.join("project1/main.py"),
            "def hello():\n    print('world')\n",
        )
        .unwrap();
        fs::write(
            base.join("project1/README.md"),
            "# Project 1\nA test project",
        )
        .unwrap();

        // Create project2 with a Rust file
        fs::create_dir(base.join("project2")).unwrap();
        fs::write(base.join("project2/lib.rs"), "fn test() {}\n").unwrap();

        temp_dir
    }

    #[test]
    fn test_extract_project_name() {
        let repo_root = Path::new("/home/user/portfolio");
        let file_path = Path::new("/home/user/portfolio/my_project/src/main.rs");

        let project_name = extract_project_name(file_path, repo_root);
        assert_eq!(project_name, Some("my_project".to_string()));
    }

    #[test]
    fn test_extract_project_name_root_file() {
        let repo_root = Path::new("/home/user/portfolio");
        let file_path = Path::new("/home/user/portfolio/README.md");

        let project_name = extract_project_name(file_path, repo_root);
        // Root level files have no project subdirectory
        assert!(project_name.is_none());
    }

    #[test]
    fn test_is_readme() {
        let temp_dir = TempDir::new().unwrap();
        let readme_path = temp_dir.path().join("README.md");
        fs::write(&readme_path, "test").unwrap();

        let entry = WalkDir::new(temp_dir.path())
            .into_iter()
            .find(|e| e.as_ref().unwrap().path() == readme_path)
            .unwrap()
            .unwrap();

        assert!(is_readme(&entry));
    }

    #[test]
    fn test_is_readme_case_insensitive() {
        let temp_dir = TempDir::new().unwrap();
        let readme_path = temp_dir.path().join("readme.MD");
        fs::write(&readme_path, "test").unwrap();

        let entry = WalkDir::new(temp_dir.path())
            .into_iter()
            .find(|e| e.as_ref().unwrap().path() == readme_path)
            .unwrap()
            .unwrap();

        assert!(is_readme(&entry));
    }

    #[test]
    fn test_run_ingestion_full_pipeline() {
        let temp_dir = create_test_workspace();
        let path = temp_dir.path().to_str().unwrap();

        let (result, _calls_map, _imports_map) = run_ingestion(path, None);

        // Should find 2 code files (main.py and lib.rs)
        assert_eq!(result.code_chunks.len(), 2);

        // Should find 1 README
        assert_eq!(result.readme_chunks.len(), 1);

        // Check Python function was extracted
        assert!(
            result
                .code_chunks
                .iter()
                .any(|c| c.identifier == "hello" && c.language == "python")
        );

        // Check Rust function was extracted
        assert!(
            result
                .code_chunks
                .iter()
                .any(|c| c.identifier == "test" && c.language == "rust")
        );

        // Check project names were set
        assert!(
            result
                .code_chunks
                .iter()
                .any(|c| c.project_name == "project1")
        );
        assert!(
            result
                .code_chunks
                .iter()
                .any(|c| c.project_name == "project2")
        );

        // Check README content
        assert_eq!(result.readme_chunks[0].project_name, "project1");
        assert!(result.readme_chunks[0].content.contains("Project 1"));

        // Check paths are relative with forward slashes
        assert!(
            result
                .code_chunks
                .iter()
                .all(|c| !c.file_path.contains('\\') && !c.file_path.starts_with('/'))
        );
    }

    #[test]
    fn test_process_code_file_enriches_metadata() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();

        fs::create_dir(base.join("myproj")).unwrap();
        let file_path = base.join("myproj/code.py");
        fs::write(&file_path, "def func(): pass\n").unwrap();

        let entry = WalkDir::new(base)
            .into_iter()
            .find(|e| e.as_ref().unwrap().path() == file_path)
            .unwrap()
            .unwrap();

        let mut analyzer = CodeAnalyzer::new();
        let processed = process_code_file(&entry, base, &mut analyzer, None);

        assert_eq!(processed.chunks.len(), 1);
        assert_eq!(processed.chunks[0].file_path, "myproj/code.py");
        assert_eq!(processed.chunks[0].project_name, "myproj");
    }

    #[test]
    fn test_resolve_project_name_cli_override() {
        let repo_root = Path::new("/home/user/portfolio");
        let file_path = Path::new("/home/user/portfolio/my_project/src/main.rs");

        // CLI override takes precedence
        let name = resolve_project_name(file_path, repo_root, Some("my-app"));
        assert_eq!(name, "my-app");
    }

    #[test]
    fn test_resolve_project_name_root_file_fallback() {
        let repo_root = Path::new("/home/user/portfolio");
        let file_path = Path::new("/home/user/portfolio/README.md");

        // Root-level file falls back to repo directory name
        let name = resolve_project_name(file_path, repo_root, None);
        assert_eq!(name, "portfolio");
    }

    #[test]
    fn test_normalize_path() {
        let repo_root = Path::new("/home/user/portfolio");
        let file_path = Path::new("/home/user/portfolio/project1/src/main.rs");

        let normalized = normalize_path(file_path, repo_root);
        assert_eq!(normalized, "project1/src/main.rs");
    }

    #[test]
    fn test_run_ingestion_typescript() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();

        fs::create_dir(base.join("webapp")).unwrap();
        fs::write(
            base.join("webapp/app.ts"),
            "function greet(name: string): string { return `Hello ${name}`; }\n",
        )
        .unwrap();
        fs::write(
            base.join("webapp/utils.tsx"),
            "const Button = (props: { label: string }) => { return <button>{props.label}</button>; };\n",
        )
        .unwrap();
        fs::write(
            base.join("webapp/legacy.js"),
            "function oldHelper() { return 1; }\n",
        )
        .unwrap();

        let path = base.to_str().unwrap();
        let (result, _calls_map, _imports_map) = run_ingestion(path, None);

        // All three files should produce chunks
        assert!(
            result
                .code_chunks
                .iter()
                .any(|c| c.identifier == "greet" && c.language == "typescript")
        );
        assert!(
            result
                .code_chunks
                .iter()
                .any(|c| c.identifier == "Button" && c.language == "typescript")
        );
        assert!(
            result
                .code_chunks
                .iter()
                .any(|c| c.identifier == "oldHelper" && c.language == "typescript")
        );

        // Paths should be normalized
        assert!(
            result
                .code_chunks
                .iter()
                .all(|c| !c.file_path.contains('\\') && !c.file_path.starts_with('/'))
        );
    }

    #[test]
    fn test_run_ingestion_with_project_name_override() {
        let temp_dir = create_test_workspace();
        let path = temp_dir.path().to_str().unwrap();

        let (result, _calls_map, _imports_map) = run_ingestion(path, Some("my-app"));

        // All chunks should have the override name
        assert!(
            result
                .code_chunks
                .iter()
                .all(|c| c.project_name == "my-app")
        );
        assert!(
            result
                .readme_chunks
                .iter()
                .all(|c| c.project_name == "my-app")
        );
    }
}
