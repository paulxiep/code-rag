//! Integration tests for incremental ingestion (V1.3)
//!
//! Tests the parse → reconcile roundtrip with real files on disk.
//! No database or embedder needed — exercises run_ingestion + reconcile.

use code_raptor::{ExistingFileIndex, IngestionResult, reconcile, run_ingestion};
use std::fs;
use tempfile::TempDir;

/// Convert an IngestionResult into an ExistingFileIndex,
/// simulating what the database would return via get_file_index().
fn simulate_stored_index(result: &IngestionResult) -> ExistingFileIndex {
    let mut index = ExistingFileIndex::default();

    for chunk in &result.code_chunks {
        index
            .code_files
            .entry(chunk.file_path.clone())
            .and_modify(|(_, ids)| ids.push(chunk.chunk_id.clone()))
            .or_insert_with(|| (chunk.content_hash.clone(), vec![chunk.chunk_id.clone()]));
    }

    for chunk in &result.readme_chunks {
        index
            .readme_files
            .entry(chunk.file_path.clone())
            .and_modify(|(_, ids)| ids.push(chunk.chunk_id.clone()))
            .or_insert_with(|| (chunk.content_hash.clone(), vec![chunk.chunk_id.clone()]));
    }

    for chunk in &result.crate_chunks {
        index.crate_entries.insert(
            chunk.crate_name.clone(),
            (chunk.content_hash.clone(), chunk.chunk_id.clone()),
        );
    }

    for chunk in &result.module_doc_chunks {
        index.module_doc_files.insert(
            chunk.file_path.clone(),
            (chunk.content_hash.clone(), chunk.chunk_id.clone()),
        );
    }

    index
}

/// Create a workspace with two projects: Python + Rust
fn create_workspace() -> TempDir {
    let temp = TempDir::new().unwrap();
    let base = temp.path();

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

    fs::create_dir(base.join("project2")).unwrap();
    fs::write(base.join("project2/lib.rs"), "fn test() {}\n").unwrap();

    temp
}

// ============================================================================
// Roundtrip: ingest → ingest again (no changes) → verify 0 inserts/deletes
// ============================================================================

#[test]
fn roundtrip_no_changes() {
    let workspace = create_workspace();
    let path = workspace.path().to_str().unwrap();

    let (result, _) = run_ingestion(path, None);
    assert!(
        !result.code_chunks.is_empty(),
        "sanity: should parse some code"
    );

    let existing = simulate_stored_index(&result);

    // Re-ingest same files — nothing changed
    let (result2, _) = run_ingestion(path, None);
    let diff = reconcile(&result2, &existing);

    assert_eq!(diff.stats.chunks_to_insert, 0);
    assert_eq!(diff.stats.chunks_to_delete, 0);
    assert_eq!(diff.stats.files_changed, 0);
    assert_eq!(diff.stats.files_new, 0);
    assert_eq!(diff.stats.files_deleted, 0);
    assert!(diff.stats.files_unchanged > 0);
}

// ============================================================================
// Modify file → ingest → verify correct file replaced
// ============================================================================

#[test]
fn detects_modified_file() {
    let workspace = create_workspace();
    let path = workspace.path().to_str().unwrap();

    let (result, _) = run_ingestion(path, None);
    let existing = simulate_stored_index(&result);

    // Modify project1/main.py (adds a new function)
    fs::write(
        workspace.path().join("project1/main.py"),
        "def hello():\n    print('modified')\n\ndef new_func():\n    pass\n",
    )
    .unwrap();

    let (result2, _) = run_ingestion(path, None);
    let diff = reconcile(&result2, &existing);

    // The modified file should show as changed
    assert!(diff.stats.files_changed >= 1);
    assert!(!diff.to_insert.code_chunks.is_empty());
    assert!(!diff.to_delete.code_chunk_ids.is_empty());

    // The unmodified file (project2/lib.rs) should not be touched
    let project2_inserts = diff
        .to_insert
        .code_chunks
        .iter()
        .filter(|c| c.file_path.contains("project2"))
        .count();
    assert_eq!(project2_inserts, 0);

    // New function count should be higher than original for project1
    let project1_new = diff
        .to_insert
        .code_chunks
        .iter()
        .filter(|c| c.file_path.contains("project1"))
        .count();
    assert!(
        project1_new >= 2,
        "modified file should produce at least 2 chunks (hello + new_func)"
    );
}

// ============================================================================
// Delete file → ingest → verify chunks removed
// ============================================================================

#[test]
fn detects_deleted_file() {
    let workspace = create_workspace();
    let path = workspace.path().to_str().unwrap();

    let (result, _) = run_ingestion(path, None);
    let existing = simulate_stored_index(&result);

    // Count how many chunks came from project2/lib.rs
    let project2_chunk_ids: Vec<_> = result
        .code_chunks
        .iter()
        .filter(|c| c.file_path.contains("project2"))
        .map(|c| c.chunk_id.clone())
        .collect();
    assert!(!project2_chunk_ids.is_empty());

    // Delete the file
    fs::remove_file(workspace.path().join("project2/lib.rs")).unwrap();

    let (result2, _) = run_ingestion(path, None);
    let diff = reconcile(&result2, &existing);

    assert!(diff.stats.files_deleted >= 1);
    // All project2 chunk IDs should be in the deletion set
    for id in &project2_chunk_ids {
        assert!(
            diff.to_delete.code_chunk_ids.contains(id),
            "deleted file's chunk_id should be in deletions"
        );
    }
    // No project2 chunks in insertions
    assert!(
        !diff
            .to_insert
            .code_chunks
            .iter()
            .any(|c| c.file_path.contains("project2"))
    );
}

// ============================================================================
// New file → ingest → verify chunks added
// ============================================================================

#[test]
fn detects_new_file() {
    let workspace = create_workspace();
    let path = workspace.path().to_str().unwrap();

    let (result, _) = run_ingestion(path, None);
    let existing = simulate_stored_index(&result);

    // Add a new file
    fs::write(
        workspace.path().join("project1/utils.py"),
        "def helper():\n    return 42\n",
    )
    .unwrap();

    let (result2, _) = run_ingestion(path, None);
    let diff = reconcile(&result2, &existing);

    assert!(diff.stats.files_new >= 1);
    assert!(
        diff.to_insert
            .code_chunks
            .iter()
            .any(|c| c.identifier == "helper"),
        "new file's function should be in insertions"
    );
    // No deletions for existing files
    assert_eq!(diff.stats.files_deleted, 0);
}

// ============================================================================
// Mixed scenario: one changed, one new, one deleted, one unchanged
// ============================================================================

#[test]
fn mixed_changes() {
    let workspace = create_workspace();
    let path = workspace.path().to_str().unwrap();

    let (result, _) = run_ingestion(path, None);
    let existing = simulate_stored_index(&result);

    // Modify project1/main.py
    fs::write(
        workspace.path().join("project1/main.py"),
        "def hello_v2():\n    print('v2')\n",
    )
    .unwrap();

    // Delete project2/lib.rs
    fs::remove_file(workspace.path().join("project2/lib.rs")).unwrap();

    // Add a new file
    fs::write(
        workspace.path().join("project1/new.py"),
        "class Foo:\n    def bar(self):\n        pass\n",
    )
    .unwrap();

    let (result2, _) = run_ingestion(path, None);
    let diff = reconcile(&result2, &existing);

    assert!(diff.stats.files_changed >= 1, "main.py was modified");
    assert!(diff.stats.files_deleted >= 1, "lib.rs was deleted");
    assert!(diff.stats.files_new >= 1, "new.py was added");
    // README was unchanged
    assert!(diff.stats.files_unchanged >= 1, "README unchanged");

    assert!(diff.stats.chunks_to_insert > 0);
    assert!(diff.stats.chunks_to_delete > 0);
}

// ============================================================================
// project_name override: all chunks get the same name, reconcile is stable
// ============================================================================

#[test]
fn project_name_override_stable_reconcile() {
    let workspace = create_workspace();
    let path = workspace.path().to_str().unwrap();

    let (result, _) = run_ingestion(path, Some("my-portfolio"));

    // All chunks should have the override name
    assert!(
        result
            .code_chunks
            .iter()
            .all(|c| c.project_name == "my-portfolio")
    );
    assert!(
        result
            .readme_chunks
            .iter()
            .all(|c| c.project_name == "my-portfolio")
    );

    // Reconcile should show no changes
    let existing = simulate_stored_index(&result);
    let (result2, _) = run_ingestion(path, Some("my-portfolio"));
    let diff = reconcile(&result2, &existing);

    assert_eq!(diff.stats.chunks_to_insert, 0);
    assert_eq!(diff.stats.chunks_to_delete, 0);
}

// ============================================================================
// Paths are normalized: relative, forward slashes
// ============================================================================

#[test]
fn paths_normalized() {
    let workspace = create_workspace();
    let path = workspace.path().to_str().unwrap();

    let (result, _) = run_ingestion(path, None);

    for chunk in &result.code_chunks {
        assert!(
            !chunk.file_path.contains('\\'),
            "path should use forward slashes: {}",
            chunk.file_path
        );
        assert!(
            !chunk.file_path.starts_with('/'),
            "path should be relative: {}",
            chunk.file_path
        );
    }
}

// ============================================================================
// Content hashes are file-level (all chunks from same file share hash)
// ============================================================================

#[test]
fn file_level_content_hash() {
    let workspace = create_workspace();
    let base = workspace.path();

    // Create a file with multiple functions
    fs::write(
        base.join("project1/multi.py"),
        "def func_a():\n    pass\n\ndef func_b():\n    pass\n\ndef func_c():\n    pass\n",
    )
    .unwrap();

    let path = base.to_str().unwrap();
    let (result, _) = run_ingestion(path, None);

    let multi_chunks: Vec<_> = result
        .code_chunks
        .iter()
        .filter(|c| c.file_path.contains("multi.py"))
        .collect();

    assert!(
        multi_chunks.len() >= 2,
        "multi.py should produce multiple chunks"
    );

    // All chunks from the same file should have the same content_hash
    let first_hash = &multi_chunks[0].content_hash;
    assert!(
        multi_chunks.iter().all(|c| &c.content_hash == first_hash),
        "all chunks from the same file must share the file-level content_hash"
    );
}

// ============================================================================
// Deterministic chunk_ids: same input → same IDs across runs
// ============================================================================

#[test]
fn deterministic_ids_stable_across_runs() {
    let workspace = create_workspace();
    let path = workspace.path().to_str().unwrap();

    let (result1, _) = run_ingestion(path, None);
    let (result2, _) = run_ingestion(path, None);

    // Same number of chunks
    assert_eq!(result1.code_chunks.len(), result2.code_chunks.len());

    // chunk_ids must be identical across runs for the same input
    let mut ids1: Vec<_> = result1.code_chunks.iter().map(|c| &c.chunk_id).collect();
    let mut ids2: Vec<_> = result2.code_chunks.iter().map(|c| &c.chunk_id).collect();
    ids1.sort();
    ids2.sort();
    assert_eq!(
        ids1, ids2,
        "code chunk_ids must be deterministic across runs"
    );

    // README chunk_ids too
    assert_eq!(result1.readme_chunks.len(), result2.readme_chunks.len());
    for (a, b) in result1
        .readme_chunks
        .iter()
        .zip(result2.readme_chunks.iter())
    {
        assert_eq!(
            a.chunk_id, b.chunk_id,
            "readme chunk_ids must be deterministic"
        );
    }
}
