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
///
/// DDG edges connect whole *statement* nodes (see
/// `graphs::pdg::object::compute_data_dependence`), while sources and
/// sinks are detected at the finer *sub-expression* granularity (a bare
/// `Identifier`/`MemberExpr` for sources, a `CallExpr` for sinks). These
/// two id spaces essentially never intersect directly, so before running
/// BFS we bridge the gap by containment: each source/sink is mapped to
/// the smallest DDG-participating statement whose byte span contains it
/// (falling back to itself when it already *is* a DDG-participating
/// statement, keeping the flat/simple case working unchanged) -- see
/// [`resolve_effective_ids`].
pub fn taint_flow_paths(cpg: &CodePropertyGraph, allow: &HashSet<String>) -> usize {
    let source_registry: HashSet<&str> = taint_sources(&cpg.language).iter().copied().collect();
    let sink_registry = effective_registry(&cpg.language, allow);
    if source_registry.is_empty() || sink_registry.is_empty() {
        return 0;
    }

    let mut forward: HashMap<&str, Vec<&str>> = HashMap::new();
    let mut ddg_stmt_ids: HashSet<&str> = HashSet::new();
    for edge in &cpg.edges {
        if edge.kind != CPGEdgeKind::Ddg {
            continue;
        }
        forward
            .entry(edge.source.as_str())
            .or_default()
            .push(edge.target.as_str());
        ddg_stmt_ids.insert(edge.source.as_str());
        ddg_stmt_ids.insert(edge.target.as_str());
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

    // `{ddg_participating_stmt_id: (start_byte, end_byte)}` -- built once
    // up front so resolving each source/sink's enclosing statement is a
    // single pass over this (typically small) set, not a scan of every
    // CPG node.
    let mut ddg_spans: HashMap<&str, (usize, usize)> = HashMap::new();
    for &stmt_id in &ddg_stmt_ids {
        if let Some(stmt_node) = cpg.nodes.get(stmt_id) {
            ddg_spans.insert(
                stmt_id,
                (stmt_node.uast.span.start_byte, stmt_node.uast.span.end_byte),
            );
        }
    }

    let effective_sources = resolve_effective_ids(sources.iter().copied(), cpg, &ddg_spans);
    let effective_sinks = resolve_effective_ids(sinks.iter().copied(), cpg, &ddg_spans);
    if effective_sources.is_empty() || effective_sinks.is_empty() {
        return 0;
    }

    let mut total = 0;
    let mut reachable_cache: HashMap<&str, HashSet<&str>> = HashMap::new();
    for &src in &sources {
        let Some(&eff_src) = effective_sources.get(src) else {
            continue;
        };
        let reachable = reachable_cache
            .entry(eff_src)
            .or_insert_with(|| bfs_reachable(&forward, eff_src));
        for &sink in &sinks {
            if let Some(&eff_sink) = effective_sinks.get(sink) {
                if reachable.contains(eff_sink) {
                    total += 1;
                }
            }
        }
    }
    total
}

/// Map each source/sink node id to its "effective" DDG-graph entry point.
///
/// A candidate that already equals a DDG-participating statement id maps
/// to itself (the flat/simple case that already worked before this
/// fix). Otherwise this finds the smallest DDG-participating statement
/// span that contains the candidate's span -- its nearest enclosing
/// statement -- since that's the node the DDG adjacency actually knows
/// how to traverse from. Ties (equal-width enclosing spans) keep the
/// first one encountered; candidates with no enclosing DDG statement
/// (e.g. dead code sliced out of the CFG) are simply omitted from the
/// result.
fn resolve_effective_ids<'a>(
    candidate_ids: impl IntoIterator<Item = &'a str>,
    cpg: &'a CodePropertyGraph,
    ddg_spans: &HashMap<&'a str, (usize, usize)>,
) -> HashMap<&'a str, &'a str> {
    let mut resolved = HashMap::new();
    for nid in candidate_ids {
        if ddg_spans.contains_key(nid) {
            resolved.insert(nid, nid);
            continue;
        }
        let Some(node) = cpg.nodes.get(nid) else {
            continue;
        };
        let (start, end) = (node.uast.span.start_byte, node.uast.span.end_byte);
        let mut best: Option<(&str, usize)> = None;
        for (&stmt_id, &(s_start, s_end)) in ddg_spans {
            if s_start <= start && end <= s_end {
                let width = s_end - s_start;
                if best.is_none_or(|(_, best_width)| width < best_width) {
                    best = Some((stmt_id, width));
                }
            }
        }
        if let Some((best_id, _)) = best {
            resolved.insert(nid, best_id);
        }
    }
    resolved
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
    fn direct_source_to_sink_flow_is_detected() {
        // Issue #154: DDG edges are statement-granular (source/target are
        // whole-statement ids from `pdg::object`), but source/sink
        // detection here matches individual sub-expression nodes (the
        // bare `input`/`eval` identifiers). `resolve_effective_ids`
        // bridges the two id spaces by containment, so this textbook
        // "assign from tainted source, use in sink" pattern is now
        // connected.
        let source = "def f():\n    x = input()\n    eval(x)\n";
        let result = parse_source(source, "python", None).unwrap();
        let cpg = CodePropertyGraph::from_uast(&result.uast_root, source);
        assert_eq!(taint_flow_paths(&cpg, &HashSet::new()), 1);
    }

    #[test]
    fn no_sources_or_sinks_is_zero() {
        let source = "x = 1 + 2\n";
        let result = parse_source(source, "python", None).unwrap();
        let cpg = CodePropertyGraph::from_uast(&result.uast_root, source);
        assert_eq!(taint_flow_paths(&cpg, &HashSet::new()), 0);
    }
}
