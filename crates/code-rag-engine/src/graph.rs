//! Call graph traversal and query augmentation for Relationship intent.
//!
//! Pure Rust, no external deps, wasm32-compatible. Used by both the server
//! retriever and the WASM standalone API.

use std::collections::{HashMap, HashSet, VecDeque};

use code_rag_types::CodeChunk;

use super::retriever::ScoredChunk;

/// In-memory call graph built from persisted edges.
/// At code-repo scale (~1500 edges), construction takes microseconds.
pub struct CallGraph {
    forward: HashMap<String, Vec<String>>, // caller -> [callees]
    reverse: HashMap<String, Vec<String>>, // callee -> [callers]
    /// Identifier → chunk_id index for target term lookup
    id_to_chunk: HashMap<String, Vec<String>>,
}

impl CallGraph {
    /// Build adjacency lists from (caller_chunk_id, callee_chunk_id) pairs.
    pub fn from_edges(edges: impl IntoIterator<Item = (String, String)>) -> Self {
        let mut forward: HashMap<String, Vec<String>> = HashMap::new();
        let mut reverse: HashMap<String, Vec<String>> = HashMap::new();

        for (caller, callee) in edges {
            forward
                .entry(caller.clone())
                .or_default()
                .push(callee.clone());
            reverse.entry(callee).or_default().push(caller);
        }

        Self {
            forward,
            reverse,
            id_to_chunk: HashMap::new(),
        }
    }

    /// Register identifier → chunk_id mappings for target term lookup.
    /// Call after construction with (identifier, chunk_id) pairs from the edge data.
    /// Deduplicates chunk_ids per identifier.
    pub fn register_identifiers(&mut self, pairs: impl IntoIterator<Item = (String, String)>) {
        for (identifier, chunk_id) in pairs {
            let entry = self
                .id_to_chunk
                .entry(identifier.to_lowercase())
                .or_default();
            if !entry.contains(&chunk_id) {
                entry.push(chunk_id);
            }
        }
    }

    /// Look up chunk IDs by identifier name (case-insensitive).
    /// Returns the unique chunk_id if exactly one chunk has this identifier,
    /// or None if ambiguous (multiple chunks) or not found.
    pub fn unique_chunk_for_identifier(&self, identifier: &str) -> Option<&str> {
        self.id_to_chunk
            .get(&identifier.to_lowercase())
            .filter(|ids| ids.len() == 1)
            .map(|ids| ids[0].as_str())
    }

    /// Direct callers of the given chunk.
    pub fn callers_of(&self, chunk_id: &str) -> &[String] {
        self.reverse
            .get(chunk_id)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Direct callees of the given chunk.
    pub fn callees_of(&self, chunk_id: &str) -> &[String] {
        self.forward
            .get(chunk_id)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// BFS from a node following the reverse (caller) direction.
    /// Returns (chunk_id, depth) pairs, excluding the start node.
    pub fn bfs_callers(&self, chunk_id: &str, max_depth: usize) -> Vec<(String, usize)> {
        self.bfs(chunk_id, max_depth, &self.reverse)
    }

    /// BFS from a node following the forward (callee) direction.
    /// Returns (chunk_id, depth) pairs, excluding the start node.
    pub fn bfs_callees(&self, chunk_id: &str, max_depth: usize) -> Vec<(String, usize)> {
        self.bfs(chunk_id, max_depth, &self.forward)
    }

    fn bfs(
        &self,
        start: &str,
        max_depth: usize,
        adj: &HashMap<String, Vec<String>>,
    ) -> Vec<(String, usize)> {
        let mut visited = HashSet::new();
        visited.insert(start.to_string());
        let mut queue = VecDeque::new();
        let mut result = Vec::new();

        if let Some(neighbors) = adj.get(start) {
            for n in neighbors {
                if visited.insert(n.clone()) {
                    queue.push_back((n.clone(), 1));
                }
            }
        }

        while let Some((node, depth)) = queue.pop_front() {
            result.push((node.clone(), depth));
            if depth < max_depth
                && let Some(neighbors) = adj.get(&node)
            {
                for n in neighbors {
                    if visited.insert(n.clone()) {
                        queue.push_back((n.clone(), depth + 1));
                    }
                }
            }
        }

        result
    }

    /// BFS shortest path from `from` to `to`. Returns the path including both endpoints,
    /// or None if no path exists. Searches in the forward direction.
    pub fn find_path(&self, from: &str, to: &str) -> Option<Vec<String>> {
        if from == to {
            return Some(vec![from.to_string()]);
        }

        let mut visited = HashSet::new();
        visited.insert(from.to_string());
        let mut queue = VecDeque::new();
        // (current_node, path_so_far)
        queue.push_back((from.to_string(), vec![from.to_string()]));

        while let Some((node, path)) = queue.pop_front() {
            if let Some(neighbors) = self.forward.get(&node) {
                for n in neighbors {
                    if n == to {
                        let mut full_path = path.clone();
                        full_path.push(n.clone());
                        return Some(full_path);
                    }
                    if visited.insert(n.clone()) {
                        let mut new_path = path.clone();
                        new_path.push(n.clone());
                        queue.push_back((n.clone(), new_path));
                    }
                }
            }
        }

        None
    }

    /// Returns true if the graph has any edges.
    pub fn is_empty(&self) -> bool {
        self.forward.is_empty()
    }
}

/// Direction of a graph query, inferred from the query text.
#[derive(Debug, Clone, PartialEq)]
pub enum GraphDirection {
    /// "What calls X?" / "called by"
    Callers,
    /// "What does X call?" / "depends on" / "uses"
    Callees,
    /// "Path from A to B" / "flow" / "trace"
    Path(String, String),
    /// Default: return both callers and callees
    Both,
}

/// Detect the intended graph traversal direction from a query string.
pub fn detect_direction(query: &str) -> GraphDirection {
    let q = query.to_lowercase();

    // "called by" → callers
    if q.contains("called by") || q.contains("invoked by") {
        return GraphDirection::Callers;
    }

    // "what calls X" / "who calls X" → callers (verb before target)
    if q.contains("what calls")
        || q.contains("who calls")
        || q.contains("where is") && q.contains("called")
    {
        return GraphDirection::Callers;
    }

    // "what does X call" / "X calls what" → callees
    if q.contains("does") && q.contains("call") || q.contains("depend on") || q.contains("uses") {
        return GraphDirection::Callees;
    }

    // "path between" / "flow" / "trace" → need two endpoints (handled by caller)
    if q.contains("path between")
        || q.contains("path from")
        || q.contains("flow")
        || q.contains("trace")
        || q.contains("chain")
    {
        // Path needs two identifiers; caller must parse them.
        // Fall through to Both for now; graph_augment will upgrade if it finds two.
        return GraphDirection::Both;
    }

    GraphDirection::Both
}

/// Common English stopwords to skip when extracting target identifiers.
const STOPWORDS: &[&str] = &[
    "the", "a", "an", "this", "that", "my", "our", "your", "its", "function", "method", "class",
    "struct", "module", "file", "code", "in", "of", "to", "from", "for", "with",
];

/// Extract the first non-stopword token from a string.
fn first_meaningful_token(s: &str) -> Option<String> {
    s.split(|c: char| c.is_whitespace() || c == '?' || c == '.' || c == '`')
        .map(|t| t.trim())
        .filter(|t| !t.is_empty())
        .find(|t| !STOPWORDS.contains(&t.to_lowercase().as_str()))
        .map(|t| t.to_string())
}

/// Extract the likely target function identifier from a query.
/// Returns the identifier string if found.
pub fn extract_target_term(query: &str) -> Option<String> {
    let q = query.to_lowercase();

    // Patterns: "what calls <target>", "who calls <target>"
    for prefix in &["what calls ", "who calls ", "called by "] {
        if let Some(rest) = q.find(prefix).map(|i| &query[i + prefix.len()..])
            && let Some(term) = first_meaningful_token(rest)
        {
            return Some(term);
        }
    }

    // "what does <target> call"
    if let Some(start) = q.find("what does ").map(|i| i + "what does ".len()) {
        let rest = &query[start..];
        if let Some(end) = rest.to_lowercase().find(" call") {
            let term = rest[..end].trim();
            if let Some(t) = first_meaningful_token(term) {
                return Some(t);
            }
        }
    }

    // "callers of <target>" / "callees of <target>"
    for prefix in &["callers of ", "callees of "] {
        if let Some(rest) = q.find(prefix).map(|i| &query[i + prefix.len()..])
            && let Some(term) = first_meaningful_token(rest)
        {
            return Some(term);
        }
    }

    // Fallback: look for backtick-quoted identifiers
    if let Some(start) = query.find('`')
        && let Some(end) = query[start + 1..].find('`')
    {
        let term = &query[start + 1..start + 1 + end];
        if !term.is_empty() {
            return Some(term.to_string());
        }
    }

    None
}

/// Result of graph augmentation: which chunks to add from graph traversal.
#[derive(Debug)]
pub struct GraphAugmentResult {
    /// The chunk ID identified as the query target
    pub target_chunk_id: String,
    /// The human-readable identifier of the target
    pub target_identifier: String,
    /// The traversal direction used
    pub direction: GraphDirection,
    /// Chunk IDs resolved from graph traversal (to be fetched by caller)
    pub resolved_chunk_ids: Vec<String>,
}

/// Identify graph-relevant chunk IDs from vector search candidates + call graph.
///
/// This is the shared core logic used by both the server retriever and WASM
/// standalone API. Callers are responsible for fetching full CodeChunks by ID
/// (platform-specific: LanceDB vs in-memory index) and wrapping them as
/// ScoredChunks.
///
/// `candidates` are (chunk_id, identifier) pairs from vector search top-N.
pub fn graph_augment(
    query: &str,
    candidates: &[(String, String)],
    graph: &CallGraph,
) -> Option<GraphAugmentResult> {
    if candidates.is_empty() || graph.is_empty() {
        return None;
    }

    // Filter out test functions from candidates — they contain query text in code
    // and rank highly in vector search but are never meaningful graph targets.
    let filtered: Vec<_> = candidates
        .iter()
        .filter(|(_, id)| !id.starts_with("test_"))
        .cloned()
        .collect();
    let candidates = if filtered.is_empty() {
        candidates
    } else {
        &filtered
    };

    // 1. Extract target term from query
    let target_term = extract_target_term(query);

    // 2. Match target term against candidate identifiers, then fall back to graph index
    let (target_chunk_id, target_identifier) = if let Some(ref term) = target_term {
        let term_lower = term.to_lowercase();

        // First: exact match in vector candidates
        let exact = candidates
            .iter()
            .find(|(_, id)| id.to_lowercase() == term_lower);

        // Second: partial match in vector candidates (identifier contains term)
        let partial = || {
            candidates
                .iter()
                .find(|(_, id)| id.to_lowercase().contains(&term_lower))
        };

        // Third: look up in graph's identifier index (catches targets not in vector top-N)
        let graph_lookup = || {
            graph
                .unique_chunk_for_identifier(&term_lower)
                .map(|cid| (cid.to_string(), term.clone()))
        };

        if let Some((cid, id)) = exact {
            (cid.clone(), id.clone())
        } else if let Some((cid, id)) = graph_lookup() {
            // Graph index exact match takes priority over partial candidate match
            (cid, id)
        } else if let Some((cid, id)) = partial() {
            (cid.clone(), id.clone())
        } else {
            // No match at all — don't guess, return None
            return None;
        }
    } else {
        // No target term extracted — use top-1 candidate
        (candidates[0].0.clone(), candidates[0].1.clone())
    };

    // 3. Detect direction
    let direction = detect_direction(query);

    // 4. Traverse graph based on direction
    let resolved_chunk_ids = match &direction {
        GraphDirection::Callers => graph.callers_of(&target_chunk_id).to_vec(),
        GraphDirection::Callees => graph.callees_of(&target_chunk_id).to_vec(),
        GraphDirection::Both => {
            let mut ids = graph.callers_of(&target_chunk_id).to_vec();
            ids.extend_from_slice(graph.callees_of(&target_chunk_id));
            // Deduplicate
            let mut seen = HashSet::new();
            ids.retain(|id| seen.insert(id.clone()));
            ids
        }
        GraphDirection::Path(from, to) => graph.find_path(from, to).unwrap_or_default(),
    };

    if resolved_chunk_ids.is_empty() {
        return None;
    }

    Some(GraphAugmentResult {
        target_chunk_id,
        target_identifier,
        direction,
        resolved_chunk_ids,
    })
}

/// Merge graph-resolved ScoredChunks into existing vector results,
/// deduplicating by chunk_id. Graph chunks that already exist in
/// `existing` are skipped. Returns the merged, deduplicated list.
pub fn merge_graph_chunks(
    existing: Vec<ScoredChunk<CodeChunk>>,
    graph_chunks: Vec<ScoredChunk<CodeChunk>>,
) -> Vec<ScoredChunk<CodeChunk>> {
    let existing_ids: HashSet<String> = existing
        .iter()
        .map(|sc| sc.chunk.chunk_id.clone())
        .collect();

    let mut merged = existing;
    for gc in graph_chunks {
        if !existing_ids.contains(&gc.chunk.chunk_id) {
            merged.push(gc);
        }
    }
    merged
}

/// Relevance score priors for graph-resolved chunks, by resolution tier.
/// These are initial scores before reranking. The reranker will override them.
pub fn tier_score(resolution_tier: u8) -> f32 {
    match resolution_tier {
        1 => 0.85, // same-file: highest confidence
        2 => 0.80, // import-based
        _ => 0.75, // unique-global or unknown
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_graph() -> CallGraph {
        // A -> B -> C
        // A -> D
        CallGraph::from_edges(vec![
            ("A".into(), "B".into()),
            ("B".into(), "C".into()),
            ("A".into(), "D".into()),
        ])
    }

    #[test]
    fn test_empty_graph() {
        let g = CallGraph::from_edges(std::iter::empty());
        assert!(g.is_empty());
        assert_eq!(g.callers_of("A"), &[] as &[String]);
        assert_eq!(g.callees_of("A"), &[] as &[String]);
    }

    #[test]
    fn test_callers_of() {
        let g = make_graph();
        assert_eq!(g.callers_of("B"), &["A".to_string()]);
        assert_eq!(g.callers_of("D"), &["A".to_string()]);
        assert_eq!(g.callers_of("A"), &[] as &[String]);
    }

    #[test]
    fn test_callees_of() {
        let g = make_graph();
        let mut callees: Vec<&str> = g.callees_of("A").iter().map(|s| s.as_str()).collect();
        callees.sort();
        assert_eq!(callees, vec!["B", "D"]);
        assert_eq!(g.callees_of("C"), &[] as &[String]);
    }

    #[test]
    fn test_bfs_callers_depth_1() {
        let g = make_graph();
        let result = g.bfs_callers("C", 1);
        assert_eq!(result, vec![("B".into(), 1)]);
    }

    #[test]
    fn test_bfs_callers_depth_2() {
        let g = make_graph();
        let result = g.bfs_callers("C", 2);
        assert_eq!(result, vec![("B".into(), 1), ("A".into(), 2)]);
    }

    #[test]
    fn test_bfs_callees() {
        let g = make_graph();
        let mut result = g.bfs_callees("A", 2);
        result.sort_by_key(|(_, d)| *d);
        assert!(result.len() == 3); // B, D at depth 1, C at depth 2
        assert!(result.iter().any(|(id, d)| id == "B" && *d == 1));
        assert!(result.iter().any(|(id, d)| id == "D" && *d == 1));
        assert!(result.iter().any(|(id, d)| id == "C" && *d == 2));
    }

    #[test]
    fn test_find_path_exists() {
        let g = make_graph();
        let path = g.find_path("A", "C");
        assert_eq!(path, Some(vec!["A".into(), "B".into(), "C".into()]));
    }

    #[test]
    fn test_find_path_direct() {
        let g = make_graph();
        let path = g.find_path("A", "B");
        assert_eq!(path, Some(vec!["A".into(), "B".into()]));
    }

    #[test]
    fn test_find_path_none() {
        let g = make_graph();
        assert_eq!(g.find_path("C", "A"), None);
    }

    #[test]
    fn test_find_path_self() {
        let g = make_graph();
        assert_eq!(g.find_path("A", "A"), Some(vec!["A".into()]));
    }

    #[test]
    fn test_detect_direction_callers() {
        assert_eq!(
            detect_direction("What calls the retrieve function?"),
            GraphDirection::Callers
        );
        assert_eq!(
            detect_direction("Who calls retrieve?"),
            GraphDirection::Callers
        );
        assert_eq!(
            detect_direction("Functions called by main"),
            GraphDirection::Callers
        );
    }

    #[test]
    fn test_detect_direction_callees() {
        assert_eq!(
            detect_direction("What does retrieve call?"),
            GraphDirection::Callees
        );
        assert_eq!(
            detect_direction("What does run_ingestion depend on?"),
            GraphDirection::Callees
        );
    }

    #[test]
    fn test_detect_direction_both() {
        assert_eq!(
            detect_direction("Tell me about the retrieve function"),
            GraphDirection::Both
        );
    }

    #[test]
    fn test_extract_target_term_what_calls() {
        assert_eq!(
            extract_target_term("What calls retrieve?"),
            Some("retrieve".into())
        );
        assert_eq!(
            extract_target_term("What calls the retrieve function?"),
            Some("retrieve".into()) // skips stopwords "the" and "function"
        );
    }

    #[test]
    fn test_extract_target_term_what_does_call() {
        assert_eq!(
            extract_target_term("What does run_ingestion call?"),
            Some("run_ingestion".into())
        );
    }

    #[test]
    fn test_extract_target_term_backtick() {
        assert_eq!(
            extract_target_term("Show callers of `retrieve`"),
            Some("retrieve".into())
        );
    }

    #[test]
    fn test_extract_target_term_callers_of() {
        assert_eq!(
            extract_target_term("callers of retrieve"),
            Some("retrieve".into())
        );
    }

    #[test]
    fn test_extract_target_term_none() {
        assert_eq!(extract_target_term("How does the system work?"), None);
    }

    #[test]
    fn test_graph_augment_empty_graph() {
        let g = CallGraph::from_edges(std::iter::empty());
        let candidates = vec![("c1".into(), "foo".into())];
        assert!(graph_augment("What calls foo?", &candidates, &g).is_none());
    }

    #[test]
    fn test_graph_augment_empty_candidates() {
        let g = make_graph();
        let candidates: Vec<(String, String)> = vec![];
        assert!(graph_augment("What calls foo?", &candidates, &g).is_none());
    }

    #[test]
    fn test_graph_augment_callers() {
        let g = make_graph();
        let candidates = vec![
            ("B".into(), "process".into()),
            ("A".into(), "main".into()),
            ("C".into(), "helper".into()),
        ];
        let result = graph_augment("What calls process?", &candidates, &g).unwrap();
        assert_eq!(result.target_chunk_id, "B");
        assert_eq!(result.resolved_chunk_ids, vec!["A".to_string()]);
    }

    #[test]
    fn test_graph_augment_callees() {
        let g = make_graph();
        let candidates = vec![("A".into(), "main".into())];
        let result = graph_augment("What does main call?", &candidates, &g).unwrap();
        assert_eq!(result.target_chunk_id, "A");
        let mut resolved = result.resolved_chunk_ids.clone();
        resolved.sort();
        assert_eq!(resolved, vec!["B".to_string(), "D".to_string()]);
    }

    #[test]
    fn test_graph_augment_no_results() {
        let g = make_graph();
        let candidates = vec![("C".into(), "leaf".into())];
        // C has no callees, and "What does leaf call?" → Callees direction
        let result = graph_augment("What does leaf call?", &candidates, &g);
        assert!(result.is_none());
    }

    #[test]
    fn test_merge_graph_chunks_no_overlap() {
        let existing = vec![ScoredChunk {
            chunk: make_code_chunk("c1", "foo"),
            score: 0.9,
        }];
        let graph = vec![ScoredChunk {
            chunk: make_code_chunk("c2", "bar"),
            score: 0.85,
        }];
        let merged = merge_graph_chunks(existing, graph);
        assert_eq!(merged.len(), 2);
    }

    #[test]
    fn test_merge_graph_chunks_with_overlap() {
        let existing = vec![ScoredChunk {
            chunk: make_code_chunk("c1", "foo"),
            score: 0.9,
        }];
        let graph = vec![ScoredChunk {
            chunk: make_code_chunk("c1", "foo"),
            score: 0.85,
        }];
        let merged = merge_graph_chunks(existing, graph);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].score, 0.9); // keeps existing score
    }

    #[test]
    fn test_tier_scores() {
        assert!(tier_score(1) > tier_score(2));
        assert!(tier_score(2) > tier_score(3));
    }

    fn make_code_chunk(chunk_id: &str, identifier: &str) -> CodeChunk {
        CodeChunk {
            file_path: "test.rs".into(),
            language: "rust".into(),
            identifier: identifier.into(),
            node_type: "function_item".into(),
            code_content: format!("fn {}() {{}}", identifier),
            start_line: 1,
            project_name: "test".into(),
            docstring: None,
            signature: None,
            chunk_id: chunk_id.into(),
            content_hash: "hash".into(),
            embedding_model_version: "test".into(),
        }
    }
}
