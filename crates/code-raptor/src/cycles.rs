//! R4: dependency-cycle detection over the file-level import graph.
//!
//! Pure (no I/O): projects `Imports` / `ReExports` edges onto a directed
//! file→file graph and enumerates its elementary circuits — the circular
//! dependencies the architecture report surfaces. File granularity falls out
//! for free: `edge_resolution` already lifts definition-level imports to
//! `source_file` / `target_file`, so the contains join is implicit. Pure
//! `Contains` edges are hierarchical (they can never close a cycle on their
//! own) and call cycles are recursion, not architecture smells — neither is a
//! cycle input.
//!
//! **Algorithm.** Iterative Tarjan SCC (Tarjan 1972) restricts the search to
//! non-trivial strongly connected components — import graphs are mostly
//! acyclic, so most components are singletons and cost nothing. Within each
//! SCC, elementary circuits are enumerated in canonical form (every cycle is
//! found exactly once, rooted at its smallest vertex) by a bounded DFS rather
//! than Johnson's (1975) blocked search: Johnson's unblocking assumes
//! *complete* exploration, which a length cap breaks — a depth-pruned blocked
//! search can leave vertices blocked and miss short cycles. The bounded DFS is
//! exact for every cycle within [`MAX_CYCLE_LEN`], and the SCC restriction
//! keeps it cheap on real import graphs.
//!
//! **Determinism.** Node indices come from the sorted file list, SCCs and
//! start vertices are processed in ascending index order, and adjacency is
//! sorted — so the canonical root of each cycle is its lexically-smallest
//! file and identical input yields identical output, caps included.

use std::collections::{HashMap, HashSet};

use code_rag_types::{EdgeRelation, GraphEdge};

/// Enumeration stops after this many cycles (cycle counts are output-
/// exponential in pathological graphs; 50 is far beyond what a report can
/// usefully show).
const MAX_CYCLES: usize = 50;
/// Circuits longer than this are skipped — a 12-file cycle is a smell of the
/// whole subsystem, not an actionable pair of files.
const MAX_CYCLE_LEN: usize = 12;

/// One circular dependency: the member files in cycle order, starting at the
/// lexically-smallest file. The closing edge (last → first) is implicit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cycle {
    pub files: Vec<String>,
}

/// Find elementary import cycles at file granularity. Output is sorted
/// `(length asc, files lex asc)`.
pub fn find_import_cycles(graph_edges: &[GraphEdge]) -> Vec<Cycle> {
    // Directed file→file edges, deduplicated; self-imports dropped.
    let mut pairs: HashSet<(&str, &str)> = HashSet::new();
    for e in graph_edges {
        if matches!(e.relation, EdgeRelation::Imports | EdgeRelation::ReExports)
            && e.source_file != e.target_file
        {
            pairs.insert((e.source_file.as_str(), e.target_file.as_str()));
        }
    }
    if pairs.is_empty() {
        return Vec::new();
    }

    // Sorted node indexing → canonical cycle root = lexically-smallest file.
    let mut files: Vec<&str> = pairs
        .iter()
        .flat_map(|&(u, v)| [u, v])
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    files.sort_unstable();
    let index: HashMap<&str, usize> = files.iter().enumerate().map(|(i, &f)| (f, i)).collect();

    let n = files.len();
    let mut adjacency: Vec<Vec<usize>> = vec![Vec::new(); n];
    for &(u, v) in &pairs {
        adjacency[index[u]].push(index[v]);
    }
    for neigh in &mut adjacency {
        neigh.sort_unstable();
    }

    // SCC restriction: cycles exist only within a strongly connected component.
    let scc_of = tarjan_scc(&adjacency);
    let mut members_by_scc: HashMap<usize, Vec<usize>> = HashMap::new();
    for (v, &c) in scc_of.iter().enumerate() {
        members_by_scc.entry(c).or_default().push(v);
    }
    let mut sccs: Vec<Vec<usize>> = members_by_scc
        .into_values()
        .filter(|m| m.len() > 1)
        .collect();
    for m in &mut sccs {
        m.sort_unstable();
    }
    sccs.sort_by_key(|m| m[0]);

    let mut cycles: Vec<Vec<usize>> = Vec::new();
    for members in &sccs {
        if cycles.len() >= MAX_CYCLES {
            break;
        }
        let member_set: HashSet<usize> = members.iter().copied().collect();
        for &start in members {
            if cycles.len() >= MAX_CYCLES {
                break;
            }
            let mut path = vec![start];
            let mut on_path: HashSet<usize> = HashSet::from([start]);
            dfs_cycles(
                &adjacency,
                &member_set,
                start,
                &mut path,
                &mut on_path,
                &mut cycles,
            );
        }
    }

    let mut out: Vec<Cycle> = cycles
        .into_iter()
        .map(|c| Cycle {
            files: c.into_iter().map(|v| files[v].to_string()).collect(),
        })
        .collect();
    out.sort_by(|a, b| {
        a.files
            .len()
            .cmp(&b.files.len())
            .then_with(|| a.files.cmp(&b.files))
    });
    out
}

/// Enumerate elementary circuits rooted at `path[0]`: only vertices `> root`
/// (canonical form — each cycle found once, at its smallest vertex) inside the
/// current SCC, depth-capped. Recursion depth ≤ [`MAX_CYCLE_LEN`].
fn dfs_cycles(
    adjacency: &[Vec<usize>],
    scc: &HashSet<usize>,
    v: usize,
    path: &mut Vec<usize>,
    on_path: &mut HashSet<usize>,
    cycles: &mut Vec<Vec<usize>>,
) {
    let root = path[0];
    for &w in &adjacency[v] {
        if cycles.len() >= MAX_CYCLES {
            return;
        }
        if w == root && path.len() >= 2 {
            cycles.push(path.clone());
        } else if w > root
            && !on_path.contains(&w)
            && scc.contains(&w)
            && path.len() < MAX_CYCLE_LEN
        {
            path.push(w);
            on_path.insert(w);
            dfs_cycles(adjacency, scc, w, path, on_path, cycles);
            path.pop();
            on_path.remove(&w);
        }
    }
}

/// Iterative Tarjan strongly-connected components. Returns the SCC id per
/// vertex (ids are arbitrary but deterministic for identical input).
fn tarjan_scc(adjacency: &[Vec<usize>]) -> Vec<usize> {
    let n = adjacency.len();
    const UNSET: usize = usize::MAX;
    let mut index = vec![UNSET; n];
    let mut low = vec![0usize; n];
    let mut on_stack = vec![false; n];
    let mut scc_of = vec![UNSET; n];
    let mut stack: Vec<usize> = Vec::new();
    let mut next_index = 0usize;
    let mut next_scc = 0usize;

    // Explicit DFS frames: (vertex, next-neighbor position).
    let mut frames: Vec<(usize, usize)> = Vec::new();
    for start in 0..n {
        if index[start] != UNSET {
            continue;
        }
        frames.push((start, 0));
        index[start] = next_index;
        low[start] = next_index;
        next_index += 1;
        stack.push(start);
        on_stack[start] = true;

        while let Some(&mut (v, ref mut pos)) = frames.last_mut() {
            if *pos < adjacency[v].len() {
                let w = adjacency[v][*pos];
                *pos += 1;
                if index[w] == UNSET {
                    index[w] = next_index;
                    low[w] = next_index;
                    next_index += 1;
                    stack.push(w);
                    on_stack[w] = true;
                    frames.push((w, 0));
                } else if on_stack[w] {
                    low[v] = low[v].min(index[w]);
                }
            } else {
                frames.pop();
                if let Some(&(parent, _)) = frames.last() {
                    low[parent] = low[parent].min(low[v]);
                }
                if low[v] == index[v] {
                    // v is an SCC root; pop its component.
                    loop {
                        let w = stack.pop().expect("tarjan stack underflow");
                        on_stack[w] = false;
                        scc_of[w] = next_scc;
                        if w == v {
                            break;
                        }
                    }
                    next_scc += 1;
                }
            }
        }
    }
    scc_of
}

#[cfg(test)]
mod tests {
    use super::*;
    use code_rag_types::{EdgeConfidence, EdgeContext};

    fn import(src_file: &str, tgt_file: &str) -> GraphEdge {
        GraphEdge {
            edge_id: format!("{src_file}->{tgt_file}"),
            source_chunk_id: format!("chunk:{src_file}"),
            target_chunk_id: format!("chunk:{tgt_file}"),
            source_identifier: src_file.into(),
            target_identifier: tgt_file.into(),
            source_file: src_file.into(),
            target_file: tgt_file.into(),
            project_name: "p".into(),
            relation: EdgeRelation::Imports,
            context: EdgeContext::None,
            confidence: EdgeConfidence::Extracted,
        }
    }

    #[test]
    fn dag_has_no_cycles() {
        let edges = vec![
            import("a.rs", "b.rs"),
            import("b.rs", "c.rs"),
            import("a.rs", "c.rs"),
        ];
        assert!(find_import_cycles(&edges).is_empty());
    }

    #[test]
    fn finds_two_and_three_cycles_once() {
        let edges = vec![
            // 2-cycle a <-> b
            import("a.rs", "b.rs"),
            import("b.rs", "a.rs"),
            // 3-cycle x -> y -> z -> x
            import("x.rs", "y.rs"),
            import("y.rs", "z.rs"),
            import("z.rs", "x.rs"),
        ];
        let cycles = find_import_cycles(&edges);
        assert_eq!(cycles.len(), 2);
        // Sorted length asc; rooted at lexically-smallest file.
        assert_eq!(cycles[0].files, vec!["a.rs", "b.rs"]);
        assert_eq!(cycles[1].files, vec!["x.rs", "y.rs", "z.rs"]);
    }

    #[test]
    fn self_import_and_duplicates_filtered() {
        let edges = vec![
            import("a.rs", "a.rs"),
            import("a.rs", "b.rs"),
            import("a.rs", "b.rs"), // duplicate pair
            import("b.rs", "a.rs"),
        ];
        let cycles = find_import_cycles(&edges);
        assert_eq!(cycles.len(), 1);
        assert_eq!(cycles[0].files, vec!["a.rs", "b.rs"]);
    }

    #[test]
    fn overlapping_cycles_in_one_scc() {
        // a -> b -> a and a -> b -> c -> a share the edge a->b.
        let edges = vec![
            import("a.rs", "b.rs"),
            import("b.rs", "a.rs"),
            import("b.rs", "c.rs"),
            import("c.rs", "a.rs"),
        ];
        let cycles = find_import_cycles(&edges);
        assert_eq!(cycles.len(), 2);
        assert_eq!(cycles[0].files, vec!["a.rs", "b.rs"]);
        assert_eq!(cycles[1].files, vec!["a.rs", "b.rs", "c.rs"]);
    }

    #[test]
    fn deterministic_across_runs_and_input_order() {
        let mut edges = vec![
            import("m.rs", "n.rs"),
            import("n.rs", "m.rs"),
            import("p.rs", "q.rs"),
            import("q.rs", "p.rs"),
        ];
        let a = find_import_cycles(&edges);
        edges.reverse();
        let b = find_import_cycles(&edges);
        assert_eq!(a, b);
        assert_eq!(a.len(), 2);
    }

    #[test]
    fn cycle_cap_respected() {
        // Complete digraph on 6 vertices has far more than MAX_CYCLES
        // elementary circuits (~400) — enumeration must stop at the cap.
        let names: Vec<String> = (0..6).map(|i| format!("f{i}.rs")).collect();
        let mut edges = Vec::new();
        for u in &names {
            for v in &names {
                if u != v {
                    edges.push(import(u, v));
                }
            }
        }
        let cycles = find_import_cycles(&edges);
        assert_eq!(cycles.len(), MAX_CYCLES);
    }
}
