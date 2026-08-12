//! R4: degree centrality — "the functions to understand first".
//!
//! Pure and WASM-safe (std only) so the native architecture report and the
//! browser demo compute identical rankings from the same exported edges. The
//! heavy analytics (betweenness, cycles) stay native in `code-raptor`; degree
//! is cheap enough to run anywhere, so it lives here as the single source of
//! truth per the Track R crate split (degree → engine, the rest → raptor).

use std::collections::HashMap;

/// One node with its weighted degree, ranked by [`degree_centrality`].
#[derive(Clone, Debug, PartialEq)]
pub struct CentralNode {
    pub id: String,
    pub degree: f64,
}

/// Weighted degree centrality over undirected `(u, v, w)` edges.
///
/// Multi-edges between the same pair are summed; self-loops are ignored (a
/// node's coupling to itself says nothing about where to start reading).
/// Output is sorted `(degree desc, id asc)` — deterministic for identical
/// input regardless of edge order.
pub fn degree_centrality(
    edges: impl IntoIterator<Item = (String, String, f64)>,
) -> Vec<CentralNode> {
    let mut degree: HashMap<String, f64> = HashMap::new();
    for (u, v, w) in edges {
        if u == v {
            continue;
        }
        *degree.entry(v).or_insert(0.0) += w;
        *degree.entry(u).or_insert(0.0) += w;
    }
    let mut out: Vec<CentralNode> = degree
        .into_iter()
        .map(|(id, degree)| CentralNode { id, degree })
        .collect();
    out.sort_by(|a, b| {
        b.degree
            .partial_cmp(&a.degree)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.id.cmp(&b.id))
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(u: &str, v: &str, w: f64) -> (String, String, f64) {
        (u.to_string(), v.to_string(), w)
    }

    #[test]
    fn ranks_hub_first() {
        // Star: hub touches a, b, c; a-b adds one more edge.
        let edges = vec![
            e("hub", "a", 1.0),
            e("hub", "b", 1.0),
            e("hub", "c", 1.0),
            e("a", "b", 1.0),
        ];
        let ranked = degree_centrality(edges);
        assert_eq!(ranked[0].id, "hub");
        assert_eq!(ranked[0].degree, 3.0);
        assert_eq!(ranked.len(), 4);
    }

    #[test]
    fn multi_edges_summed_self_loops_ignored() {
        let edges = vec![e("a", "b", 1.0), e("b", "a", 2.0), e("a", "a", 10.0)];
        let ranked = degree_centrality(edges);
        assert_eq!(ranked.len(), 2);
        assert_eq!(ranked[0].id, "a"); // tie on degree 3.0 → id asc
        assert_eq!(ranked[0].degree, 3.0);
        assert_eq!(ranked[1].degree, 3.0);
    }

    #[test]
    fn deterministic_under_input_order() {
        let fwd = vec![e("x", "y", 1.0), e("y", "z", 2.0), e("x", "z", 1.5)];
        let mut rev = fwd.clone();
        rev.reverse();
        assert_eq!(degree_centrality(fwd), degree_centrality(rev));
    }

    #[test]
    fn tie_breaks_by_id() {
        let edges = vec![e("b", "c", 1.0), e("a", "d", 1.0)];
        let ranked = degree_centrality(edges);
        let ids: Vec<&str> = ranked.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(ids, vec!["a", "b", "c", "d"]);
    }
}
