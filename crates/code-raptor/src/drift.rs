//! R5: architecture drift — emergent communities vs the folder layout.
//!
//! Pure (no I/O): given one project's community assignments + member chunks,
//! compare the bottom-up partition against the top-down directory tree and
//! surface where they diverge. Folder identity is the parent directory of each
//! member's `file_path` — the same derivation as `clusterchunk::dominant_dir`
//! — so no edge join is needed and MCP can reuse this from persisted
//! `community_assignments` + `get_chunks_by_ids` alone. The comparison is
//! meaningful because folder→file `contains` edges are deliberately excluded
//! from partitioning (see `topology.rs`): agreement here is earned, not
//! recovered from the input.

use std::collections::BTreeMap;
use std::collections::HashMap;

use code_rag_types::CodeChunk;

use crate::cluster::CommunityResult;

/// Groups smaller than this never produce a divergence (a 2-member community
/// spanning 2 dirs is noise, not drift).
const MIN_GROUP_SIZE: usize = 3;
/// Purity / concentration below this flags a divergence.
const DIVERGENCE_THRESHOLD: f64 = 0.6;
/// Ranked divergences kept in the report.
const TOP_DIVERGENCES: usize = 8;

/// How one community maps onto the directory tree.
#[derive(Debug, Clone)]
pub struct CommunityDrift {
    pub community_id: u32,
    /// Members counted (project-local members present in the chunk map).
    pub size: usize,
    /// The directory most members live in (tie → longer, then lexically
    /// smallest — the `dominant_dir` rule).
    pub dominant_dir: String,
    /// Members in the dominant directory / size.
    pub purity: f64,
    /// Every directory the community spans, sorted.
    pub dirs: Vec<String>,
}

/// How one directory maps onto the communities.
#[derive(Debug, Clone)]
pub struct FolderDrift {
    pub dir: String,
    /// Members counted in this directory.
    pub size: usize,
    /// The community most members belong to (tie → smaller id).
    pub dominant_community: u32,
    /// Members in the dominant community / size.
    pub concentration: f64,
    /// Every community present in the directory, sorted.
    pub communities: Vec<u32>,
}

/// One ranked divergence between the emergent and folder views.
#[derive(Debug, Clone)]
pub enum Divergence {
    /// A community whose members scatter across directories.
    ScatteredCommunity(CommunityDrift),
    /// A directory that splits into multiple communities.
    FragmentedFolder(FolderDrift),
}

impl Divergence {
    /// Impact score: how impure × how big. Comparable across both kinds.
    pub fn score(&self) -> f64 {
        match self {
            Divergence::ScatteredCommunity(c) => (1.0 - c.purity) * c.size as f64,
            Divergence::FragmentedFolder(f) => (1.0 - f.concentration) * f.size as f64,
        }
    }
}

/// The full emergent-vs-folder comparison for one project.
#[derive(Debug, Clone, Default)]
pub struct DriftReport {
    /// All communities, sorted by `community_id`.
    pub communities: Vec<CommunityDrift>,
    /// All directories, sorted by `dir`. The report renders only the flagged
    /// divergences; the full list is for the MCP-facing insights facade.
    pub folders: Vec<FolderDrift>,
    /// Top divergences, ranked by `(score desc, kind, id)`.
    pub divergences: Vec<Divergence>,
    /// Size-weighted mean community purity (1.0 when there are no members).
    pub mean_purity: f64,
}

/// Parent directory of a forward-slash path (`""` for a bare filename).
fn parent_dir(path: &str) -> &str {
    path.rfind('/').map(|i| &path[..i]).unwrap_or("")
}

/// Compare one project's communities against its directory layout.
///
/// Members missing from `members` or belonging to another project are skipped
/// (same cross-project rule as `analytics::compute`).
pub fn compare(
    project: &str,
    results: &[CommunityResult],
    members: &HashMap<String, CodeChunk>,
) -> DriftReport {
    // community -> dir -> count and dir -> community -> count. BTreeMaps so
    // every downstream iteration is ordered without extra sorts.
    let mut by_community: BTreeMap<u32, BTreeMap<&str, usize>> = BTreeMap::new();
    let mut by_dir: BTreeMap<&str, BTreeMap<u32, usize>> = BTreeMap::new();
    for r in results {
        let Some(chunk) = members.get(&r.chunk_id) else {
            continue;
        };
        if chunk.project_name != project {
            continue;
        }
        let dir = parent_dir(&chunk.file_path);
        *by_community
            .entry(r.community_id)
            .or_default()
            .entry(dir)
            .or_insert(0) += 1;
        *by_dir
            .entry(dir)
            .or_default()
            .entry(r.community_id)
            .or_insert(0) += 1;
    }

    let mut communities: Vec<CommunityDrift> = Vec::with_capacity(by_community.len());
    let mut dominant_total = 0usize;
    let mut member_total = 0usize;
    for (&community_id, dirs) in &by_community {
        let size: usize = dirs.values().sum();
        // Dominant dir: count desc, then longer dir, then lexically smallest —
        // the `clusterchunk::dominant_dir` tie-break.
        let (dominant_dir, dominant_count) = dirs
            .iter()
            .max_by(|a, b| {
                a.1.cmp(b.1)
                    .then(a.0.len().cmp(&b.0.len()))
                    .then(b.0.cmp(a.0))
            })
            .map(|(d, c)| (d.to_string(), *c))
            .unwrap_or_default();
        dominant_total += dominant_count;
        member_total += size;
        communities.push(CommunityDrift {
            community_id,
            size,
            dominant_dir,
            purity: dominant_count as f64 / size as f64,
            dirs: dirs.keys().map(|d| d.to_string()).collect(),
        });
    }

    let mut folders: Vec<FolderDrift> = Vec::with_capacity(by_dir.len());
    for (dir, comms) in &by_dir {
        let size: usize = comms.values().sum();
        // Dominant community: count desc, tie → smaller id (BTreeMap order +
        // strict `>` keeps the first maximum).
        let (dominant_community, dominant_count) = comms.iter().fold(
            (0u32, 0usize),
            |best, (&id, &c)| {
                if c > best.1 { (id, c) } else { best }
            },
        );
        folders.push(FolderDrift {
            dir: dir.to_string(),
            size,
            dominant_community,
            concentration: dominant_count as f64 / size as f64,
            communities: comms.keys().copied().collect(),
        });
    }

    let mut divergences: Vec<Divergence> = Vec::new();
    for c in &communities {
        if c.size >= MIN_GROUP_SIZE && c.purity < DIVERGENCE_THRESHOLD {
            divergences.push(Divergence::ScatteredCommunity(c.clone()));
        }
    }
    for f in &folders {
        if f.size >= MIN_GROUP_SIZE && f.concentration < DIVERGENCE_THRESHOLD {
            divergences.push(Divergence::FragmentedFolder(f.clone()));
        }
    }
    // Score desc; ties broken by kind (scattered first) then id, so the
    // ordering is total and byte-stable.
    divergences.sort_by(|a, b| {
        b.score()
            .total_cmp(&a.score())
            .then_with(|| divergence_key(a).cmp(&divergence_key(b)))
    });
    divergences.truncate(TOP_DIVERGENCES);

    DriftReport {
        communities,
        folders,
        divergences,
        mean_purity: if member_total == 0 {
            1.0
        } else {
            dominant_total as f64 / member_total as f64
        },
    }
}

/// Total tie-break key: scattered communities before fragmented folders.
fn divergence_key(d: &Divergence) -> (u8, String) {
    match d {
        Divergence::ScatteredCommunity(c) => (0, c.community_id.to_string()),
        Divergence::FragmentedFolder(f) => (1, f.dir.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn code(id: &str, file: &str) -> CodeChunk {
        CodeChunk {
            file_path: file.into(),
            language: "rust".into(),
            identifier: format!("fn_{id}"),
            node_type: "function_item".into(),
            code_content: "..".into(),
            start_line: 1,
            project_name: "p".into(),
            docstring: None,
            signature: None,
            chunk_id: id.into(),
            content_hash: "h".into(),
            embedding_model_version: "test".into(),
        }
    }

    fn result(id: &str, community: u32) -> CommunityResult {
        CommunityResult {
            chunk_id: id.into(),
            community_id: community,
            cohesion: 0.5,
        }
    }

    /// Two communities, each fully inside its own dir → pure, no divergence.
    #[test]
    fn aligned_communities_produce_no_divergence() {
        let members: HashMap<String, CodeChunk> = [
            ("a", "src/alpha/a.rs"),
            ("b", "src/alpha/b.rs"),
            ("c", "src/alpha/c.rs"),
            ("d", "src/beta/d.rs"),
            ("e", "src/beta/e.rs"),
            ("f", "src/beta/f.rs"),
        ]
        .iter()
        .map(|(id, f)| (id.to_string(), code(id, f)))
        .collect();
        let results = vec![
            result("a", 0),
            result("b", 0),
            result("c", 0),
            result("d", 1),
            result("e", 1),
            result("f", 1),
        ];
        let drift = compare("p", &results, &members);
        assert_eq!(drift.communities.len(), 2);
        assert_eq!(drift.folders.len(), 2);
        assert!(drift.divergences.is_empty());
        assert_eq!(drift.mean_purity, 1.0);
        assert_eq!(drift.communities[0].dominant_dir, "src/alpha");
        assert_eq!(drift.communities[0].purity, 1.0);
        assert_eq!(drift.folders[0].dir, "src/alpha");
        assert_eq!(drift.folders[0].dominant_community, 0);
    }

    /// One community scattered evenly over three dirs → ScatteredCommunity;
    /// the dirs each also host a second community → FragmentedFolder too.
    #[test]
    fn scattered_community_is_flagged() {
        let members: HashMap<String, CodeChunk> = [
            ("a", "src/alpha/a.rs"),
            ("b", "src/beta/b.rs"),
            ("c", "src/gamma/c.rs"),
        ]
        .iter()
        .map(|(id, f)| (id.to_string(), code(id, f)))
        .collect();
        let results = vec![result("a", 0), result("b", 0), result("c", 0)];
        let drift = compare("p", &results, &members);
        assert_eq!(drift.divergences.len(), 1);
        match &drift.divergences[0] {
            Divergence::ScatteredCommunity(c) => {
                assert_eq!(c.community_id, 0);
                assert_eq!(c.size, 3);
                assert_eq!(c.dirs, vec!["src/alpha", "src/beta", "src/gamma"]);
                assert!((c.purity - 1.0 / 3.0).abs() < 1e-9);
            }
            other => panic!("expected ScatteredCommunity, got {other:?}"),
        }
        assert!(drift.mean_purity < DIVERGENCE_THRESHOLD);
    }

    /// One dir splitting into three communities → FragmentedFolder.
    #[test]
    fn fragmented_folder_is_flagged() {
        let members: HashMap<String, CodeChunk> = ["a", "b", "c"]
            .iter()
            .map(|id| (id.to_string(), code(id, "src/hub/x.rs")))
            .collect();
        let results = vec![result("a", 0), result("b", 1), result("c", 2)];
        let drift = compare("p", &results, &members);
        // Each community is a pure singleton (below MIN_GROUP_SIZE anyway);
        // the folder is the divergence.
        assert_eq!(drift.divergences.len(), 1);
        match &drift.divergences[0] {
            Divergence::FragmentedFolder(f) => {
                assert_eq!(f.dir, "src/hub");
                assert_eq!(f.communities, vec![0, 1, 2]);
                assert_eq!(f.dominant_community, 0);
            }
            other => panic!("expected FragmentedFolder, got {other:?}"),
        }
    }

    /// Groups under MIN_GROUP_SIZE never flag, however impure.
    #[test]
    fn tiny_groups_do_not_flag() {
        let members: HashMap<String, CodeChunk> = [("a", "src/x/a.rs"), ("b", "src/y/b.rs")]
            .iter()
            .map(|(id, f)| (id.to_string(), code(id, f)))
            .collect();
        let results = vec![result("a", 0), result("b", 0)];
        let drift = compare("p", &results, &members);
        assert!(drift.divergences.is_empty());
        assert_eq!(drift.communities[0].purity, 0.5);
    }

    /// Foreign-project and unknown members are skipped entirely.
    #[test]
    fn foreign_and_missing_members_skipped() {
        let mut foreign = code("x", "other/x.rs");
        foreign.project_name = "other".into();
        let members: HashMap<String, CodeChunk> = [
            ("a".to_string(), code("a", "src/a.rs")),
            ("x".to_string(), foreign),
        ]
        .into_iter()
        .collect();
        let results = vec![result("a", 0), result("x", 0), result("missing", 0)];
        let drift = compare("p", &results, &members);
        assert_eq!(drift.communities.len(), 1);
        assert_eq!(drift.communities[0].size, 1);
    }

    #[test]
    fn empty_input_yields_default() {
        let drift = compare("p", &[], &HashMap::new());
        assert!(drift.communities.is_empty());
        assert!(drift.folders.is_empty());
        assert!(drift.divergences.is_empty());
        assert_eq!(drift.mean_purity, 1.0);
    }

    #[test]
    fn deterministic_across_runs() {
        let members: HashMap<String, CodeChunk> = [
            ("a", "src/alpha/a.rs"),
            ("b", "src/beta/b.rs"),
            ("c", "src/gamma/c.rs"),
            ("d", "src/alpha/d.rs"),
        ]
        .iter()
        .map(|(id, f)| (id.to_string(), code(id, f)))
        .collect();
        let results = vec![
            result("a", 0),
            result("b", 0),
            result("c", 0),
            result("d", 1),
        ];
        let one = compare("p", &results, &members);
        let two = compare("p", &results, &members);
        assert_eq!(format!("{one:?}"), format!("{two:?}"));
    }
}
