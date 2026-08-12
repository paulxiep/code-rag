//! R4: architecture report rendering — `ProjectAnalytics` → markdown.
//!
//! Pure (no I/O): returns the report as a `String`; `lib.rs` decides where it
//! lands on disk. Deliberately timestamp-free so identical input renders
//! byte-identical output (the project's determinism standard — same reason the
//! cluster summaries hash a canonical tuple). Style follows the harness report
//! family (`src/harness/report.rs`): `std::fmt::Write` into a `String`,
//! markdown tables for enumerable facts.

use std::fmt::Write;

use crate::analytics::{Bridge, CommunityLine, ProjectAnalytics};
use crate::drift::{Divergence, DriftReport};

/// Render the architecture report for one project.
pub fn render_markdown(
    project: &str,
    analytics: &ProjectAnalytics,
    communities: &[CommunityLine],
    drift: &DriftReport,
    suggested_questions: &[String],
) -> String {
    let mut s = String::new();
    let w = &mut s;

    let _ = writeln!(w, "# Architecture report: {project}");
    let _ = writeln!(w);
    let _ = writeln!(
        w,
        "Emergent topology over {} nodes and {} relation edges, partitioned into {} communities.",
        analytics.node_count,
        analytics.edge_count,
        communities.len()
    );

    let _ = writeln!(w);
    let _ = writeln!(w, "## Read these first");
    let _ = writeln!(w);
    if analytics.central_nodes.is_empty() {
        let _ = writeln!(w, "No code nodes in the topology.");
    } else {
        let _ = writeln!(
            w,
            "The most-connected definitions — understanding these unlocks the most of the codebase."
        );
        let _ = writeln!(w);
        let _ = writeln!(w, "| Definition | File | Degree | Community |");
        let _ = writeln!(w, "|---|---|---|---|");
        for c in &analytics.central_nodes {
            let _ = writeln!(
                w,
                "| `{}` | {} | {} | {} |",
                c.identifier,
                c.file,
                c.degree,
                c.community_id
                    .map(|id| id.to_string())
                    .unwrap_or_else(|| "-".to_string()),
            );
        }
    }

    let _ = writeln!(w);
    let _ = writeln!(w, "## Communities");
    let _ = writeln!(w);
    if communities.is_empty() {
        let _ = writeln!(w, "No communities detected.");
    } else {
        let _ = writeln!(w, "| Id | Size | Cohesion | Likely concern | Directory |");
        let _ = writeln!(w, "|---|---|---|---|---|");
        for c in communities {
            let _ = writeln!(
                w,
                "| {} | {} | {:.2} | {} | {} |",
                c.id,
                c.size,
                c.cohesion,
                code_or_dash(&c.central_member),
                text_or_dash(&c.dominant_dir),
            );
        }
    }

    let _ = writeln!(w);
    let _ = writeln!(w, "## Emergent vs folder structure");
    let _ = writeln!(w);
    if drift.communities.is_empty() {
        let _ = writeln!(w, "No communities to compare.");
    } else {
        let _ = writeln!(
            w,
            "Bottom-up communities compared against the top-down directory layout \
             (folder edges are excluded from partitioning, so agreement here is \
             earned, not assumed). Mean community purity (size-weighted): {:.2}.",
            drift.mean_purity
        );
        if drift.divergences.is_empty() {
            let _ = writeln!(w);
            let _ = writeln!(
                w,
                "Communities align with the folder layout — no significant drift."
            );
        } else {
            let scattered: Vec<_> = drift
                .divergences
                .iter()
                .filter_map(|d| match d {
                    Divergence::ScatteredCommunity(c) => Some(c),
                    _ => None,
                })
                .collect();
            let fragmented: Vec<_> = drift
                .divergences
                .iter()
                .filter_map(|d| match d {
                    Divergence::FragmentedFolder(f) => Some(f),
                    _ => None,
                })
                .collect();
            if !scattered.is_empty() {
                let _ = writeln!(w);
                let _ = writeln!(w, "Communities whose members scatter across directories:");
                let _ = writeln!(w);
                let _ = writeln!(
                    w,
                    "| Community | Size | Dominant directory | Purity | Spans |"
                );
                let _ = writeln!(w, "|---|---|---|---|---|");
                for c in scattered {
                    let _ = writeln!(
                        w,
                        "| {} | {} | {} | {:.2} | {} |",
                        c.community_id,
                        c.size,
                        text_or_dash(&c.dominant_dir),
                        c.purity,
                        listed(&c.dirs),
                    );
                }
            }
            if !fragmented.is_empty() {
                let _ = writeln!(w);
                let _ = writeln!(w, "Directories that split into multiple communities:");
                let _ = writeln!(w);
                let _ = writeln!(
                    w,
                    "| Directory | Members | Dominant community | Concentration | Communities |"
                );
                let _ = writeln!(w, "|---|---|---|---|---|");
                for f in fragmented {
                    let ids: Vec<String> = f.communities.iter().map(|id| id.to_string()).collect();
                    let _ = writeln!(
                        w,
                        "| {} | {} | {} | {:.2} | {} |",
                        text_or_dash(&f.dir),
                        f.size,
                        f.dominant_community,
                        f.concentration,
                        listed(&ids),
                    );
                }
            }
        }
    }

    let _ = writeln!(w);
    let _ = writeln!(w, "## Cross-module bridges");
    let _ = writeln!(w);
    if analytics.bridges.is_empty() {
        let _ = writeln!(
            w,
            "No cross-community edges — communities are fully separated."
        );
    } else {
        let _ = writeln!(
            w,
            "Edges carrying the most shortest-path traffic *between* communities — hidden coupling."
        );
        let _ = writeln!(w);
        write_bridge_table(w, &analytics.bridges, "Betweenness", |b| {
            format!("{:.1}", b.betweenness)
        });
    }

    let _ = writeln!(w);
    let _ = writeln!(w, "## Surprising connections");
    let _ = writeln!(w);
    if analytics.surprising.is_empty() {
        let _ = writeln!(w, "None.");
    } else {
        let _ = writeln!(
            w,
            "Bridges re-ranked by unexpectedness (betweenness × weight / links between the pair): \
             one of only a few edges tying two otherwise-separate communities together."
        );
        let _ = writeln!(w);
        write_bridge_table(w, &analytics.surprising, "Surprise", |b| {
            format!("{:.1}", b.surprise)
        });
    }

    let _ = writeln!(w);
    let _ = writeln!(w, "## Dependency cycles");
    let _ = writeln!(w);
    if analytics.cycles.is_empty() {
        // A positive signal, worth stating explicitly.
        let _ = writeln!(w, "None detected — the file-level import graph is acyclic.");
    } else {
        for c in &analytics.cycles {
            let mut path = c.files.join(" → ");
            if let Some(first) = c.files.first() {
                let _ = write!(path, " → {first}");
            }
            let _ = writeln!(w, "- {path}");
        }
    }

    let _ = writeln!(w);
    let _ = writeln!(w, "## Questions this topology can answer");
    let _ = writeln!(w);
    if suggested_questions.is_empty() {
        let _ = writeln!(w, "None — the topology is empty.");
    } else {
        for q in suggested_questions {
            let _ = writeln!(w, "- {q}");
        }
    }

    s
}

/// A capped comma list: first four entries, then `+N more`.
fn listed(items: &[String]) -> String {
    const SHOWN: usize = 4;
    let mut out = items
        .iter()
        .take(SHOWN)
        .map(|s| if s.is_empty() { "-" } else { s.as_str() })
        .collect::<Vec<_>>()
        .join(", ");
    if items.len() > SHOWN {
        let _ = write!(out, " +{} more", items.len() - SHOWN);
    }
    out
}

/// Deterministic question templates instantiated from the data; templates
/// whose source list is empty are skipped.
pub fn suggested_questions(
    analytics: &ProjectAnalytics,
    communities: &[CommunityLine],
    drift: &DriftReport,
) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(c) = analytics.central_nodes.first() {
        out.push(format!(
            "What does `{}` do, and what depends on it?",
            c.identifier
        ));
    }
    if let Some(c) = communities
        .iter()
        .max_by_key(|c| (c.size, std::cmp::Reverse(c.id)))
        && !c.central_member.is_empty()
    {
        out.push(format!(
            "What is the subsystem around `{}` (community {}) responsible for?",
            c.central_member, c.id
        ));
    }
    if let Some(b) = analytics.surprising.first() {
        out.push(format!(
            "Why are `{}` and `{}` coupled across module boundaries ({})?",
            b.source.identifier,
            b.target.identifier,
            relation_tags(b),
        ));
    }
    if let Some(c) = analytics.cycles.first() {
        out.push(format!(
            "What would it take to break the circular dependency {}?",
            c.files.join(" → ")
        ));
    }
    if let Some(d) = drift.divergences.first() {
        out.push(match d {
            Divergence::ScatteredCommunity(c) => format!(
                "Why does community {} (around `{}`) span {} directories?",
                c.community_id,
                c.dominant_dir,
                c.dirs.len()
            ),
            Divergence::FragmentedFolder(f) => format!(
                "Why does `{}` split into {} separate communities?",
                f.dir,
                f.communities.len()
            ),
        });
    }
    out
}

fn write_bridge_table(
    w: &mut String,
    bridges: &[Bridge],
    score_header: &str,
    score: impl Fn(&Bridge) -> String,
) {
    let _ = writeln!(
        w,
        "| From | To | Relation | {score_header} | Weight | Communities | Pair edges |"
    );
    let _ = writeln!(w, "|---|---|---|---|---|---|---|");
    for b in bridges {
        let _ = writeln!(
            w,
            "| `{}` ({}) | `{}` ({}) | {} | {} | {} | {} ↔ {} | {} |",
            code_str_or_dash(&b.source.identifier),
            text_or_dash(&b.source.file),
            code_str_or_dash(&b.target.identifier),
            text_or_dash(&b.target.file),
            relation_tags(b),
            score(b),
            b.weight,
            b.communities.0,
            b.communities.1,
            b.pair_edge_count,
        );
    }
}

/// A bridge's relation tags joined `calls+references`, `-` if unknown (a
/// topology edge with no surviving raw-edge record would be a bug upstream).
fn relation_tags(b: &Bridge) -> String {
    if b.relations.is_empty() {
        "-".to_string()
    } else {
        b.relations.join("+")
    }
}

fn text_or_dash(s: &str) -> &str {
    if s.is_empty() { "-" } else { s }
}

fn code_or_dash(s: &str) -> String {
    if s.is_empty() {
        "-".to_string()
    } else {
        format!("`{s}`")
    }
}

fn code_str_or_dash(s: &str) -> &str {
    if s.is_empty() { "-" } else { s }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analytics::compute;
    use crate::cluster;
    use crate::topology::Topology;
    use code_rag_types::{CallEdge, CodeChunk};
    use std::collections::HashMap;

    fn call(a: &str, b: &str) -> CallEdge {
        CallEdge {
            edge_id: format!("{a}->{b}"),
            caller_chunk_id: a.into(),
            callee_chunk_id: b.into(),
            caller_identifier: a.into(),
            callee_identifier: b.into(),
            caller_file: "x.rs".into(),
            callee_file: "x.rs".into(),
            project_name: "p".into(),
            resolution_tier: 1,
        }
    }

    fn code(id: &str) -> CodeChunk {
        CodeChunk {
            file_path: "src/x.rs".into(),
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

    fn rendered() -> String {
        let calls = vec![
            call("a", "b"),
            call("b", "c"),
            call("a", "c"),
            call("d", "e"),
            call("e", "f"),
            call("d", "f"),
            call("c", "d"),
        ];
        let members: HashMap<String, CodeChunk> = ["a", "b", "c", "d", "e", "f"]
            .iter()
            .map(|id| (id.to_string(), code(id)))
            .collect();
        let topo = Topology::build(&calls, &[]);
        let results = cluster::detect(&topo);
        let ccs =
            crate::clusterchunk::build_cluster_chunks("p", &topo, &results, &members, &calls, &[]);
        let analytics = compute("p", &topo, &results, &calls, &[], &members);
        let lines = crate::analytics::community_lines("p", &topo, &ccs, &members);
        let drift = crate::drift::compare("p", &results, &members);
        let questions = suggested_questions(&analytics, &lines, &drift);
        render_markdown("p", &analytics, &lines, &drift, &questions)
    }

    #[test]
    fn all_sections_present() {
        let md = rendered();
        for header in [
            "# Architecture report: p",
            "## Read these first",
            "## Communities",
            "## Emergent vs folder structure",
            "## Cross-module bridges",
            "## Surprising connections",
            "## Dependency cycles",
            "## Questions this topology can answer",
        ] {
            assert!(md.contains(header), "missing {header}");
        }
        // No import edges in the fixture → acyclic fallback is the positive path.
        assert!(md.contains("None detected"));
        // Central identifiers surface with their member labels.
        assert!(md.contains("`fn_"));
        // Every bridge row states its relation provenance.
        assert!(md.contains("| Relation |"));
        assert!(md.contains("| calls |"));
    }

    #[test]
    fn render_is_byte_deterministic() {
        assert_eq!(rendered(), rendered());
    }

    #[test]
    fn empty_analytics_render_fallbacks() {
        let analytics = compute(
            "empty",
            &Topology::build(&[], &[]),
            &[],
            &[],
            &[],
            &HashMap::new(),
        );
        let drift = crate::drift::DriftReport::default();
        let questions = suggested_questions(&analytics, &[], &drift);
        assert!(questions.is_empty());
        let md = render_markdown("empty", &analytics, &[], &drift, &questions);
        assert!(md.contains("No code nodes in the topology."));
        assert!(md.contains("No communities detected."));
        assert!(md.contains("No communities to compare."));
        assert!(md.contains("None — the topology is empty."));
    }

    #[test]
    fn questions_reference_the_data() {
        let md = rendered();
        assert!(md.contains("What does `fn_"));
        assert!(md.contains("responsible for?"));
    }
}
