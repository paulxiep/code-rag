use std::collections::{HashMap, HashSet};

use super::IngestionResult;

/// Existing file index from the database (per table).
/// Built by querying VectorStore.get_file_index() for each table.
#[derive(Debug, Default)]
pub struct ExistingFileIndex {
    /// file_path → (file_content_hash, Vec<chunk_id>) for code_chunks
    pub code_files: HashMap<String, (String, Vec<String>)>,
    /// file_path → (file_content_hash, Vec<chunk_id>) for readme_chunks
    pub readme_files: HashMap<String, (String, Vec<String>)>,
    /// crate_name → (content_hash, chunk_id) for crate_chunks
    pub crate_entries: HashMap<String, (String, String)>,
    /// file_path → (content_hash, chunk_id) for module_doc_chunks
    pub module_doc_files: HashMap<String, (String, String)>,
    /// folder_path → (content_hash, chunk_id) for folder_chunks (A2)
    pub folder_entries: HashMap<String, (String, String)>,
    /// file_path → (content_hash, chunk_id) for file_chunks (A4)
    pub file_entries: HashMap<String, (String, String)>,
}

/// Output of reconcile() — what to insert and what to delete.
#[derive(Debug)]
pub struct ReconcileResult {
    pub to_insert: IngestionResult,
    pub to_delete: DeletionsByTable,
    pub stats: IngestionStats,
}

/// Deletions partitioned by LanceDB table (each chunk type lives in its own table).
#[derive(Debug, Default)]
pub struct DeletionsByTable {
    pub code_chunk_ids: Vec<String>,
    pub readme_chunk_ids: Vec<String>,
    pub crate_chunk_ids: Vec<String>,
    pub module_doc_chunk_ids: Vec<String>,
    pub folder_chunk_ids: Vec<String>,
    pub file_chunk_ids: Vec<String>,
}

#[derive(Debug, Default)]
pub struct IngestionStats {
    pub files_unchanged: usize,
    pub files_changed: usize,
    pub files_new: usize,
    pub files_deleted: usize,
    pub chunks_to_insert: usize,
    pub chunks_to_delete: usize,
}

/// Reconcile current ingestion against existing database state.
/// Pure data comparison — no I/O, no DB handle, fully testable.
pub fn reconcile(current: &IngestionResult, existing: &ExistingFileIndex) -> ReconcileResult {
    let mut to_insert = IngestionResult::default();
    let mut to_delete = DeletionsByTable::default();
    let mut stats = IngestionStats::default();

    // Code chunks: group by file_path, compare file hash
    reconcile_by_file(
        &current.code_chunks,
        |c| &c.file_path,
        |c| &c.content_hash,
        |c| &c.chunk_id,
        &existing.code_files,
        &mut to_insert.code_chunks,
        &mut to_delete.code_chunk_ids,
        &mut stats,
    );

    // README chunks: 1:1 by file_path
    reconcile_by_file(
        &current.readme_chunks,
        |c| &c.file_path,
        |c| &c.content_hash,
        |c| &c.chunk_id,
        &existing.readme_files,
        &mut to_insert.readme_chunks,
        &mut to_delete.readme_chunk_ids,
        &mut stats,
    );

    // Module doc chunks: 1:1 by file_path
    reconcile_single_per_file(
        &current.module_doc_chunks,
        |c| &c.file_path,
        |c| &c.content_hash,
        |c| &c.chunk_id,
        &existing.module_doc_files,
        &mut to_insert.module_doc_chunks,
        &mut to_delete.module_doc_chunk_ids,
        &mut stats,
    );

    // Crate chunks: by crate_name (not file_path)
    reconcile_crates(
        &current.crate_chunks,
        &existing.crate_entries,
        &mut to_insert.crate_chunks,
        &mut to_delete.crate_chunk_ids,
        &mut stats,
    );

    // Folder summary chunks (A2): 1:1 by folder_path
    reconcile_single_per_file(
        &current.folder_chunks,
        |c| &c.folder_path,
        |c| &c.content_hash,
        |c| &c.chunk_id,
        &existing.folder_entries,
        &mut to_insert.folder_chunks,
        &mut to_delete.folder_chunk_ids,
        &mut stats,
    );

    // File summary chunks (A4): 1:1 by file_path
    reconcile_single_per_file(
        &current.file_chunks,
        |c| &c.file_path,
        |c| &c.content_hash,
        |c| &c.chunk_id,
        &existing.file_entries,
        &mut to_insert.file_chunks,
        &mut to_delete.file_chunk_ids,
        &mut stats,
    );

    stats.chunks_to_insert = to_insert.code_chunks.len()
        + to_insert.readme_chunks.len()
        + to_insert.crate_chunks.len()
        + to_insert.module_doc_chunks.len()
        + to_insert.folder_chunks.len()
        + to_insert.file_chunks.len();
    stats.chunks_to_delete = to_delete.code_chunk_ids.len()
        + to_delete.readme_chunk_ids.len()
        + to_delete.crate_chunk_ids.len()
        + to_delete.module_doc_chunk_ids.len()
        + to_delete.folder_chunk_ids.len()
        + to_delete.file_chunk_ids.len();

    ReconcileResult {
        to_insert,
        to_delete,
        stats,
    }
}

/// Reconcile chunks grouped by file_path (many chunks per file, e.g. CodeChunk).
/// Compares file-level content hash. If hash matches → skip all. If differs → nuke + replace.
///
/// `get_id` is used to subtract new chunk IDs from the delete list when a
/// changed file's new chunks share IDs with old chunks (deterministic IDs:
/// unchanged function bodies → same chunk_id). Without this subtraction, the
/// orchestrator's insert-then-delete order would clobber the just-upserted
/// row whenever a chunk_id collided with an old one.
fn reconcile_by_file<T: Clone>(
    current_chunks: &[T],
    get_path: impl Fn(&T) -> &str,
    get_hash: impl Fn(&T) -> &str,
    get_id: impl Fn(&T) -> &str,
    existing: &HashMap<String, (String, Vec<String>)>,
    insert_buf: &mut Vec<T>,
    delete_buf: &mut Vec<String>,
    stats: &mut IngestionStats,
) {
    // Group current chunks by file_path
    let mut current_by_file: HashMap<&str, Vec<&T>> = HashMap::new();
    for chunk in current_chunks {
        current_by_file
            .entry(get_path(chunk))
            .or_default()
            .push(chunk);
    }

    // Track which existing files are still present
    let mut seen_files: HashSet<&str> = HashSet::new();

    for (file_path, chunks) in &current_by_file {
        seen_files.insert(file_path);

        match existing.get(*file_path) {
            Some((stored_hash, old_ids)) => {
                // File exists in DB — compare hash
                let current_hash = get_hash(chunks[0]);
                if current_hash == stored_hash {
                    stats.files_unchanged += 1;
                } else {
                    // Changed: delete old chunks not present in the new set,
                    // upsert all new chunks. Subtracting new IDs from the
                    // delete set protects chunks whose code body is unchanged
                    // (same deterministic chunk_id) from being clobbered by
                    // the insert-then-delete order in run_incremental_ingestion.
                    stats.files_changed += 1;
                    let new_ids: HashSet<&str> = chunks.iter().map(|c| get_id(c)).collect();
                    delete_buf.extend(
                        old_ids
                            .iter()
                            .filter(|id| !new_ids.contains(id.as_str()))
                            .cloned(),
                    );
                    insert_buf.extend(chunks.iter().map(|c| (*c).clone()));
                }
            }
            None => {
                // New file: insert all
                stats.files_new += 1;
                insert_buf.extend(chunks.iter().map(|c| (*c).clone()));
            }
        }
    }

    // Orphaned: files in DB but not on disk
    for (file_path, (_, old_ids)) in existing {
        if !seen_files.contains(file_path.as_str()) {
            stats.files_deleted += 1;
            delete_buf.extend(old_ids.iter().cloned());
        }
    }
}

/// Reconcile chunks with 1:1 file mapping (e.g. ModuleDocChunk, FolderChunk,
/// FileChunk — one chunk per path).
///
/// `get_id` lets the caller skip deleting the old chunk when the new chunk's
/// deterministic id matches it (e.g. a CodeChunk for an unchanged function in
/// a changed file): without this guard the orchestrator's insert-then-delete
/// order would clobber the just-upserted row. For ModuleDocChunk in
/// particular this is the latent case where a file's `//!` doc is unchanged
/// but the surrounding source bytes shifted (chunk_id is derived from
/// doc_content, content_hash from full file).
fn reconcile_single_per_file<T: Clone>(
    current_chunks: &[T],
    get_path: impl Fn(&T) -> &str,
    get_hash: impl Fn(&T) -> &str,
    get_id: impl Fn(&T) -> &str,
    existing: &HashMap<String, (String, String)>,
    insert_buf: &mut Vec<T>,
    delete_buf: &mut Vec<String>,
    stats: &mut IngestionStats,
) {
    let mut seen: HashSet<&str> = HashSet::new();

    for chunk in current_chunks {
        let path = get_path(chunk);
        seen.insert(path);

        match existing.get(path) {
            Some((stored_hash, old_id)) => {
                if get_hash(chunk) == stored_hash {
                    stats.files_unchanged += 1;
                } else {
                    stats.files_changed += 1;
                    if get_id(chunk) != old_id.as_str() {
                        delete_buf.push(old_id.clone());
                    }
                    insert_buf.push(chunk.clone());
                }
            }
            None => {
                stats.files_new += 1;
                insert_buf.push(chunk.clone());
            }
        }
    }

    for (path, (_, old_id)) in existing {
        if !seen.contains(path.as_str()) {
            stats.files_deleted += 1;
            delete_buf.push(old_id.clone());
        }
    }
}

/// Reconcile crate chunks by crate_name (not file_path).
/// Same id-subtract guard as `reconcile_single_per_file`: when a crate's
/// metadata changes but the deterministic chunk_id happens to coincide with
/// the stored one, skip the delete so the upsert isn't clobbered.
fn reconcile_crates(
    current_chunks: &[code_rag_types::CrateChunk],
    existing: &HashMap<String, (String, String)>,
    insert_buf: &mut Vec<code_rag_types::CrateChunk>,
    delete_buf: &mut Vec<String>,
    stats: &mut IngestionStats,
) {
    let mut seen: HashSet<&str> = HashSet::new();

    for chunk in current_chunks {
        seen.insert(&chunk.crate_name);

        match existing.get(&chunk.crate_name) {
            Some((stored_hash, old_id)) => {
                if chunk.content_hash == *stored_hash {
                    stats.files_unchanged += 1;
                } else {
                    stats.files_changed += 1;
                    if chunk.chunk_id != *old_id {
                        delete_buf.push(old_id.clone());
                    }
                    insert_buf.push(chunk.clone());
                }
            }
            None => {
                stats.files_new += 1;
                insert_buf.push(chunk.clone());
            }
        }
    }

    for (crate_name, (_, old_id)) in existing {
        if !seen.contains(crate_name.as_str()) {
            stats.files_deleted += 1;
            delete_buf.push(old_id.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use code_rag_types::{
        CodeChunk, CrateChunk, FileChunk, FolderChunk, ModuleDocChunk, ReadmeChunk, new_chunk_id,
    };

    const MODEL: &str = "BGESmallENV15_384";

    fn code_chunk(file: &str, id: &str, hash: &str) -> CodeChunk {
        CodeChunk {
            file_path: file.into(),
            language: "rust".into(),
            identifier: id.into(),
            node_type: "function_item".into(),
            code_content: format!("fn {}() {{}}", id),
            start_line: 1,
            project_name: "test".into(),
            docstring: None,
            signature: None,
            chunk_id: new_chunk_id(),
            content_hash: hash.into(),
            embedding_model_version: MODEL.into(),
        }
    }

    fn readme_chunk(file: &str, hash: &str) -> ReadmeChunk {
        ReadmeChunk {
            file_path: file.into(),
            project_name: "test".into(),
            content: "# Test".into(),
            chunk_id: new_chunk_id(),
            content_hash: hash.into(),
            embedding_model_version: MODEL.into(),
        }
    }

    fn crate_chunk(name: &str, hash: &str) -> CrateChunk {
        CrateChunk {
            crate_name: name.into(),
            crate_path: format!("crates/{}", name),
            description: None,
            dependencies: vec![],
            project_name: "test".into(),
            chunk_id: new_chunk_id(),
            content_hash: hash.into(),
            embedding_model_version: MODEL.into(),
        }
    }

    fn folder_chunk(folder: &str, id: &str, hash: &str) -> FolderChunk {
        FolderChunk {
            folder_path: folder.into(),
            project_name: "test".into(),
            file_count: 0,
            languages: vec![],
            key_types: vec![],
            key_functions: vec![],
            subfolders: vec![],
            summary_text: format!("Folder: {}", folder),
            chunk_id: id.into(),
            content_hash: hash.into(),
            embedding_model_version: MODEL.into(),
        }
    }

    fn file_chunk(file: &str, id: &str, hash: &str) -> FileChunk {
        FileChunk {
            file_path: file.into(),
            project_name: "test".into(),
            language: "rust".into(),
            exports: vec![],
            imports: vec![],
            purpose: None,
            summary_text: format!("File: {}", file),
            chunk_id: id.into(),
            content_hash: hash.into(),
            embedding_model_version: MODEL.into(),
        }
    }

    fn module_doc_chunk(file: &str, hash: &str) -> ModuleDocChunk {
        ModuleDocChunk {
            file_path: file.into(),
            module_name: "test_mod".into(),
            doc_content: "//! doc".into(),
            project_name: "test".into(),
            chunk_id: new_chunk_id(),
            content_hash: hash.into(),
            embedding_model_version: MODEL.into(),
        }
    }

    #[test]
    fn test_reconcile_no_changes() {
        let current = IngestionResult {
            code_chunks: vec![
                code_chunk("src/lib.rs", "foo", "hash_a"),
                code_chunk("src/lib.rs", "bar", "hash_a"),
            ],
            readme_chunks: vec![readme_chunk("README.md", "hash_r")],
            crate_chunks: vec![crate_chunk("my-crate", "hash_c")],
            module_doc_chunks: vec![module_doc_chunk("src/lib.rs", "hash_m")],
            folder_chunks: vec![],
            file_chunks: vec![],
        };

        let existing = ExistingFileIndex {
            code_files: HashMap::from([(
                "src/lib.rs".into(),
                ("hash_a".into(), vec!["old-1".into(), "old-2".into()]),
            )]),
            readme_files: HashMap::from([(
                "README.md".into(),
                ("hash_r".into(), vec!["old-r".into()]),
            )]),
            crate_entries: HashMap::from([("my-crate".into(), ("hash_c".into(), "old-c".into()))]),
            module_doc_files: HashMap::from([(
                "src/lib.rs".into(),
                ("hash_m".into(), "old-m".into()),
            )]),
            folder_entries: HashMap::default(),
            file_entries: HashMap::default(),
        };

        let result = reconcile(&current, &existing);

        assert!(result.to_insert.code_chunks.is_empty());
        assert!(result.to_insert.readme_chunks.is_empty());
        assert!(result.to_insert.crate_chunks.is_empty());
        assert!(result.to_insert.module_doc_chunks.is_empty());
        assert!(result.to_delete.code_chunk_ids.is_empty());
        assert_eq!(result.stats.files_unchanged, 4); // 1 code file + 1 readme + 1 crate + 1 module_doc
        assert_eq!(result.stats.chunks_to_insert, 0);
        assert_eq!(result.stats.chunks_to_delete, 0);
    }

    #[test]
    fn test_reconcile_changed_file_does_not_clobber_unchanged_chunks() {
        // Regression: chunk_id is deterministic from (path, code_content),
        // so unchanged function bodies in a changed file produce the same
        // chunk_id as their existing DB row. The orchestrator runs insert
        // before delete; before this fix, the delete pass clobbered the
        // just-upserted unchanged chunks because their IDs were still in
        // the old_ids list. Verify the unchanged-ID is excluded from the
        // delete buffer.
        let mut keep = code_chunk("src/lib.rs", "kept", "hash_NEW");
        keep.chunk_id = "stable-id-kept".into();
        let mut new_one = code_chunk("src/lib.rs", "added", "hash_NEW");
        new_one.chunk_id = "fresh-id-added".into();

        let current = IngestionResult {
            code_chunks: vec![keep, new_one],
            ..Default::default()
        };

        let existing = ExistingFileIndex {
            code_files: HashMap::from([(
                "src/lib.rs".into(),
                (
                    "hash_OLD".into(),
                    vec!["stable-id-kept".into(), "old-id-removed".into()],
                ),
            )]),
            ..Default::default()
        };

        let result = reconcile(&current, &existing);

        assert_eq!(result.to_insert.code_chunks.len(), 2);
        // Only the genuinely-removed chunk gets deleted; the stable-id one
        // stays because the upsert path will refresh it in place.
        assert_eq!(result.to_delete.code_chunk_ids, vec!["old-id-removed"]);
        assert_eq!(result.stats.files_changed, 1);
    }

    #[test]
    fn test_reconcile_folder_chunks_unchanged_changed_new_deleted() {
        // Combined coverage: a folder unchanged, one changed, one new, one
        // orphaned. Verifies the FolderChunk path through reconcile().
        let current = IngestionResult {
            folder_chunks: vec![
                folder_chunk("repo/src", "id-src", "hash_src"),
                folder_chunk("repo/src/engine", "id-engine-NEW", "hash_engine_NEW"),
                folder_chunk("repo/src/added", "id-added", "hash_added"),
            ],
            ..Default::default()
        };

        let existing = ExistingFileIndex {
            folder_entries: HashMap::from([
                ("repo/src".into(), ("hash_src".into(), "id-src".into())),
                (
                    "repo/src/engine".into(),
                    ("hash_engine_OLD".into(), "id-engine-OLD".into()),
                ),
                (
                    "repo/src/orphan".into(),
                    ("hash_orphan".into(), "id-orphan".into()),
                ),
            ]),
            ..Default::default()
        };

        let result = reconcile(&current, &existing);

        // Inserts: changed (engine) + new (added) = 2; unchanged is skipped.
        assert_eq!(result.to_insert.folder_chunks.len(), 2);
        // Deletes: changed (id-engine-OLD, since new id differs) + orphan = 2.
        let mut deletes = result.to_delete.folder_chunk_ids.clone();
        deletes.sort();
        assert_eq!(deletes, vec!["id-engine-OLD", "id-orphan"]);
        // Stats roll-up.
        assert_eq!(result.stats.files_unchanged, 1);
        assert_eq!(result.stats.files_changed, 1);
        assert_eq!(result.stats.files_new, 1);
        assert_eq!(result.stats.files_deleted, 1);
    }

    #[test]
    fn test_reconcile_file_chunk_changed_skips_self_delete_when_id_collides() {
        // FileChunk's chunk_id is deterministic from (file_path, summary_text).
        // If only the canonical metadata (content_hash input) changes but
        // summary_text stays identical, chunk_id is unchanged. Make sure the
        // delete pass doesn't clobber the just-upserted row.
        let current = IngestionResult {
            file_chunks: vec![file_chunk("repo/src/lib.rs", "stable-file-id", "hash_NEW")],
            ..Default::default()
        };

        let existing = ExistingFileIndex {
            file_entries: HashMap::from([(
                "repo/src/lib.rs".into(),
                ("hash_OLD".into(), "stable-file-id".into()),
            )]),
            ..Default::default()
        };

        let result = reconcile(&current, &existing);

        assert_eq!(result.to_insert.file_chunks.len(), 1);
        assert!(result.to_delete.file_chunk_ids.is_empty());
        assert_eq!(result.stats.files_changed, 1);
    }

    #[test]
    fn test_reconcile_one_file_changed() {
        let current = IngestionResult {
            code_chunks: vec![
                code_chunk("src/lib.rs", "foo", "hash_NEW"),
                code_chunk("src/lib.rs", "bar", "hash_NEW"),
            ],
            ..Default::default()
        };

        let existing = ExistingFileIndex {
            code_files: HashMap::from([(
                "src/lib.rs".into(),
                ("hash_OLD".into(), vec!["old-1".into(), "old-2".into()]),
            )]),
            ..Default::default()
        };

        let result = reconcile(&current, &existing);

        assert_eq!(result.to_insert.code_chunks.len(), 2);
        assert_eq!(result.to_delete.code_chunk_ids.len(), 2);
        assert_eq!(result.stats.files_changed, 1);
    }

    #[test]
    fn test_reconcile_file_deleted() {
        let current = IngestionResult::default(); // no code files on disk

        let existing = ExistingFileIndex {
            code_files: HashMap::from([(
                "src/removed.rs".into(),
                (
                    "hash_x".into(),
                    vec!["old-1".into(), "old-2".into(), "old-3".into()],
                ),
            )]),
            ..Default::default()
        };

        let result = reconcile(&current, &existing);

        assert!(result.to_insert.code_chunks.is_empty());
        assert_eq!(result.to_delete.code_chunk_ids.len(), 3);
        assert_eq!(result.stats.files_deleted, 1);
    }

    #[test]
    fn test_reconcile_new_file() {
        let current = IngestionResult {
            code_chunks: vec![code_chunk("src/new.rs", "new_fn", "hash_n")],
            ..Default::default()
        };

        let existing = ExistingFileIndex::default(); // empty DB

        let result = reconcile(&current, &existing);

        assert_eq!(result.to_insert.code_chunks.len(), 1);
        assert!(result.to_delete.code_chunk_ids.is_empty());
        assert_eq!(result.stats.files_new, 1);
    }

    #[test]
    fn test_reconcile_crate_by_name() {
        let current = IngestionResult {
            crate_chunks: vec![crate_chunk("my-crate", "hash_NEW")],
            ..Default::default()
        };

        let existing = ExistingFileIndex {
            crate_entries: HashMap::from([(
                "my-crate".into(),
                ("hash_OLD".into(), "old-c".into()),
            )]),
            ..Default::default()
        };

        let result = reconcile(&current, &existing);

        assert_eq!(result.to_insert.crate_chunks.len(), 1);
        assert_eq!(result.to_delete.crate_chunk_ids, vec!["old-c"]);
        assert_eq!(result.stats.files_changed, 1);
    }

    #[test]
    fn test_reconcile_crate_removed() {
        let current = IngestionResult::default();

        let existing = ExistingFileIndex {
            crate_entries: HashMap::from([(
                "removed-crate".into(),
                ("hash_x".into(), "old-c".into()),
            )]),
            ..Default::default()
        };

        let result = reconcile(&current, &existing);

        assert_eq!(result.to_delete.crate_chunk_ids, vec!["old-c"]);
        assert_eq!(result.stats.files_deleted, 1);
    }

    #[test]
    fn test_reconcile_mixed_scenario() {
        // Scenario: 3 files — 1 unchanged, 1 changed, 1 new; 1 deleted
        let current = IngestionResult {
            code_chunks: vec![
                code_chunk("src/unchanged.rs", "a", "hash_same"),
                code_chunk("src/changed.rs", "b", "hash_NEW"),
                code_chunk("src/new.rs", "c", "hash_new"),
            ],
            ..Default::default()
        };

        let existing = ExistingFileIndex {
            code_files: HashMap::from([
                (
                    "src/unchanged.rs".into(),
                    ("hash_same".into(), vec!["id-a".into()]),
                ),
                (
                    "src/changed.rs".into(),
                    ("hash_OLD".into(), vec!["id-b".into()]),
                ),
                (
                    "src/deleted.rs".into(),
                    ("hash_del".into(), vec!["id-d".into()]),
                ),
            ]),
            ..Default::default()
        };

        let result = reconcile(&current, &existing);

        assert_eq!(result.stats.files_unchanged, 1);
        assert_eq!(result.stats.files_changed, 1);
        assert_eq!(result.stats.files_new, 1);
        assert_eq!(result.stats.files_deleted, 1);
        // Insert: changed + new = 2
        assert_eq!(result.to_insert.code_chunks.len(), 2);
        // Delete: changed old + deleted = 2
        assert_eq!(result.to_delete.code_chunk_ids.len(), 2);
    }
}
