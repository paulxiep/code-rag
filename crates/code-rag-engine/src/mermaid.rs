//! R5: Mermaid rendering for call paths.
//!
//! Pure (no I/O, wasm32-safe) so the MCP `code_rag_path` tool and the browser
//! demo render identical diagrams. Pairs with `graph::path_augment`: the hop
//! chain it resolves becomes a `flowchart LR`.

use std::fmt::Write;

/// One hop of a call path, ready to render.
#[derive(Debug, Clone, PartialEq)]
pub struct PathStep {
    /// chunk_id (not rendered; kept so callers can zip diagram ↔ data).
    pub id: String,
    /// The definition's identifier (node title).
    pub label: String,
    /// File shown under the title (callers usually pass a basename).
    pub file: String,
}

/// Render a call path as a Mermaid `flowchart LR`.
///
/// Node ids are positional (`n0..nN`) so arbitrary identifier characters can
/// never break the graph syntax; labels/files are entity-escaped. A single
/// step renders as one node with no arrows; empty input renders `""`.
pub fn render_call_path(steps: &[PathStep]) -> String {
    if steps.is_empty() {
        return String::new();
    }
    let mut s = String::new();
    let w = &mut s;
    let _ = writeln!(w, "flowchart LR");
    for (i, step) in steps.iter().enumerate() {
        let label = escape(&step.label);
        if step.file.is_empty() {
            let _ = writeln!(w, "    n{i}[\"{label}\"]");
        } else {
            let _ = writeln!(
                w,
                "    n{i}[\"{label}<br/><small>{}</small>\"]",
                escape(&step.file)
            );
        }
    }
    for i in 1..steps.len() {
        let _ = writeln!(w, "    n{} --> n{}", i - 1, i);
    }
    s
}

/// Escape label text for a double-quoted Mermaid node. `&` first, then the
/// characters Mermaid or its HTML labels would otherwise interpret.
fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('[', "&#91;")
        .replace(']', "&#93;")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(id: &str, label: &str, file: &str) -> PathStep {
        PathStep {
            id: id.into(),
            label: label.into(),
            file: file.into(),
        }
    }

    #[test]
    fn golden_three_step_path() {
        let steps = [
            step("c1", "handle_request", "server.rs"),
            step("c2", "retrieve", "retriever.rs"),
            step("c3", "rerank", "rerank.rs"),
        ];
        assert_eq!(
            render_call_path(&steps),
            "flowchart LR\n\
             \x20   n0[\"handle_request<br/><small>server.rs</small>\"]\n\
             \x20   n1[\"retrieve<br/><small>retriever.rs</small>\"]\n\
             \x20   n2[\"rerank<br/><small>rerank.rs</small>\"]\n\
             \x20   n0 --> n1\n\
             \x20   n1 --> n2\n"
        );
    }

    #[test]
    fn single_step_renders_node_without_arrows() {
        let md = render_call_path(&[step("c1", "main", "main.rs")]);
        assert!(md.contains("n0[\"main<br/><small>main.rs</small>\"]"));
        assert!(!md.contains("-->"));
    }

    #[test]
    fn labels_are_escaped() {
        let md = render_call_path(&[step("c1", "Vec<T> & \"quoted\" [x]", "")]);
        assert!(md.contains("n0[\"Vec&lt;T&gt; &amp; &quot;quoted&quot; &#91;x&#93;\"]"));
        assert!(!md.contains("<T>"));
    }

    #[test]
    fn empty_input_renders_empty() {
        assert_eq!(render_call_path(&[]), "");
    }

    #[test]
    fn deterministic_across_runs() {
        let steps = [step("a", "x", "f.rs"), step("b", "y", "g.rs")];
        assert_eq!(render_call_path(&steps), render_call_path(&steps));
    }
}
