//! Taint-flow probe (CPG → ℝ).
//!
//! A *taint flow* is a source → sink data-flow path along DDG edges in
//! the CPG, optionally interrupted by a sanitizer. v1 implements a
//! purely syntactic version: every input-like identifier is marked a
//! *source* and every dangerous-API call site a *sink*; the probe
//! counts DDG paths between them.
//!
//! Per-language source/sink registries are intentionally tiny — refine
//! when real applications surface false negatives.

use std::collections::{HashMap, HashSet};

use super::danger::{callee_from_text, effective_registry, matches_registry};
use crate::graphs::cpg::models::CPGEdgeKind;
use crate::graphs::cpg::object::CodePropertyGraph;

/// Names whose value should be treated as untrusted input.
fn taint_sources(language: &str) -> &'static [&'static str] {
    match language {
        "python" => &[
            "input",
            "sys.argv",
            "request.args",
            "request.form",
            "request.json",
            "os.environ",
        ],
        "javascript" => &[
            "process.argv",
            "process.env",
            "req.body",
            "req.query",
            "document.location",
            "window.location",
        ],
        "typescript" => &["process.argv", "process.env", "req.body", "req.query"],
        "rust" => &["std::env::args", "std::env::var"],
        "cpp" => &["argv", "getenv", "scanf"],
        "go" => &[
            "os.Getenv",
            "os.Args",
            "r.FormValue",
            "r.URL",
            "flag.String",
        ],
        _ => &[],
    }
}

/// Count DDG paths from any taint source to any dangerous-API sink.
///
/// A DDG path here is a chain of CPG nodes connected by DDG edges; this
/// counts distinct `(source_node, sink_node)` pairs that are reachable.
pub fn taint_flow_paths(cpg: &CodePropertyGraph, allow: &HashSet<String>) -> usize {
    let source_registry: HashSet<&str> = taint_sources(&cpg.language).iter().copied().collect();
    let sink_registry = effective_registry(&cpg.language, allow);
    if source_registry.is_empty() || sink_registry.is_empty() {
        return 0;
    }

    let mut forward: HashMap<&str, Vec<&str>> = HashMap::new();
    for edge in &cpg.edges {
        if edge.kind != CPGEdgeKind::Ddg {
            continue;
        }
        forward
            .entry(edge.source.as_str())
            .or_default()
            .push(edge.target.as_str());
    }
    if forward.is_empty() {
        return 0;
    }

    let mut sources = Vec::new();
    let mut sinks = HashSet::new();
    for (id, node) in &cpg.nodes {
        let text = cpg.node_text(node);
        if text.is_empty() {
            continue;
        }
        let snippet = text.trim();
        if node.kind() == "CallExpr" {
            let callee = callee_from_text(snippet);
            if !callee.is_empty() && matches_registry(&callee, sink_registry.iter().copied()) {
                sinks.insert(id.as_str());
            }
        }
        if matches!(node.kind(), "Identifier" | "MemberExpr") && source_registry.contains(snippet) {
            sources.push(id.as_str());
        }
    }
    if sources.is_empty() || sinks.is_empty() {
        return 0;
    }

    sources
        .iter()
        .map(|&src| {
            let reachable = bfs_reachable(&forward, src);
            sinks.iter().filter(|s| reachable.contains(**s)).count()
        })
        .sum()
}

fn bfs_reachable<'a>(adj: &HashMap<&'a str, Vec<&'a str>>, start: &'a str) -> HashSet<&'a str> {
    let mut visited = HashSet::from([start]);
    let mut frontier = vec![start];
    while let Some(node) = frontier.pop() {
        for &next in adj.get(node).into_iter().flatten() {
            if visited.insert(next) {
                frontier.push(next);
            }
        }
    }
    visited
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphs::ast::dispatch::parse_source;

    #[test]
    fn direct_source_to_sink_flow_is_not_detected_today() {
        // Documents a real limitation (filed as a bug, see this module's
        // doc comment above `taint_flow_paths`), not a passing case: DDG
        // edges are statement-granular (source/target are whole-statement
        // ids from `pdg::object`), but source/sink detection here matches
        // individual sub-expression nodes (the bare `input`/`eval`
        // identifiers). The two id spaces essentially never intersect, so
        // even this textbook "assign from tainted source, use in sink"
        // pattern is not connected. Ported faithfully from Python, which
        // has the identical mismatch — this is not a Rust regression.
        let source = "def f():\n    x = input()\n    eval(x)\n";
        let result = parse_source(source, "python", None).unwrap();
        let cpg = CodePropertyGraph::from_uast(&result.uast_root, source);
        assert_eq!(taint_flow_paths(&cpg, &HashSet::new()), 0);
    }

    #[test]
    fn no_sources_or_sinks_is_zero() {
        let source = "x = 1 + 2\n";
        let result = parse_source(source, "python", None).unwrap();
        let cpg = CodePropertyGraph::from_uast(&result.uast_root, source);
        assert_eq!(taint_flow_paths(&cpg, &HashSet::new()), 0);
    }
}
