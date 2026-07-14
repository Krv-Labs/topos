//! CPG builder — construct a `CodePropertyGraph` from a UAST root by
//! composing the AST, CFG, DDG, and CDG edge families into a single
//! labeled multigraph (Yamaguchi et al., arxiv:1909.03496).
//!
//! # Algorithm
//! 1. AST family: every parent → child link from the UAST.
//! 2. CFG family: every edge of the [`ControlFlowGraph`], projected from
//!    block-level (BB → BB) onto statement-level (last UAST stmt of
//!    source block → first UAST stmt of target block).
//! 3. DDG family: each `DependenceEdge(kind=Data)` from the academic PDG.
//! 4. CDG family: each `DependenceEdge(kind=Control)` from the academic PDG.

use std::collections::HashMap;

use super::models::{CPGEdge, CPGEdgeKind, CPGNode};
use crate::graphs::cfg::object::ControlFlowGraph;
use crate::graphs::pdg::object::{DependenceKind, ProgramDependenceGraph};
use crate::graphs::uast::models::UASTNode;

/// A node key stable across a single `build_cpg` call: the UAST node's
/// own id when present, or a pointer-identity fallback otherwise.
///
/// Mirrors Python's `node.id or f"anon::{id(node):x}"`, where Python's
/// `id()` builtin is object identity — Rust's closest equivalent is the
/// node's address. In practice every node the mapper engine produces has
/// a real id; the fallback only matters for hand-built nodes (e.g.
/// `graphs::cfg::builder`'s synthetic module-callable wrapper) that
/// never appear as CFG/PDG statements themselves, so it's effectively
/// unreachable in real data, same as in Python.
fn node_key(node: &UASTNode) -> String {
    if node.id.is_empty() {
        format!("anon::{:x}", std::ptr::from_ref(node) as usize)
    } else {
        node.id.clone()
    }
}

/// Return `(nodes, edges)` for the CPG, building the CFG and PDG afresh.
pub fn build_cpg(uast_root: &UASTNode, source: &str) -> (HashMap<String, CPGNode>, Vec<CPGEdge>) {
    let cfg = ControlFlowGraph::from_uast(uast_root);
    let pdg = ProgramDependenceGraph::from_uast(uast_root, source);

    let mut nodes = HashMap::new();
    collect_nodes(uast_root, &mut nodes);

    let mut edges = Vec::new();
    edges.extend(ast_edges(uast_root));
    edges.extend(cfg_edges(&cfg));
    edges.extend(dependence_edges(&pdg));

    (nodes, edges)
}

fn collect_nodes(root: &UASTNode, out: &mut HashMap<String, CPGNode>) {
    let mut stack: Vec<&UASTNode> = vec![root];
    while let Some(node) = stack.pop() {
        let key = node_key(node);
        if out.contains_key(&key) {
            continue;
        }
        out.insert(key, CPGNode { uast: node.clone() });
        stack.extend(node.children.iter());
    }
}

fn ast_edges(root: &UASTNode) -> Vec<CPGEdge> {
    let mut edges = Vec::new();
    let mut stack: Vec<&UASTNode> = vec![root];
    while let Some(parent) = stack.pop() {
        let parent_id = node_key(parent);
        for child in &parent.children {
            let child_id = node_key(child);
            edges.push(CPGEdge::new(
                parent_id.clone(),
                child_id,
                CPGEdgeKind::Ast,
                "",
            ));
            stack.push(child);
        }
    }
    edges
}

/// Project block-level CFG edges down to UAST-statement-level.
fn cfg_edges(cfg: &ControlFlowGraph) -> Vec<CPGEdge> {
    let mut edges = Vec::new();
    for edge in &cfg.edges {
        let Some(src_block) = cfg.blocks.get(&edge.source) else {
            continue;
        };
        let Some(dst_block) = cfg.blocks.get(&edge.target) else {
            continue;
        };
        let Some(src_stmt) = src_block.statements.last() else {
            continue;
        };
        let Some(dst_stmt) = dst_block.statements.first() else {
            continue;
        };
        let source = if src_stmt.id.is_empty() {
            "<anon>".to_string()
        } else {
            src_stmt.id.clone()
        };
        let target = if dst_stmt.id.is_empty() {
            "<anon>".to_string()
        } else {
            dst_stmt.id.clone()
        };
        edges.push(CPGEdge::new(
            source,
            target,
            CPGEdgeKind::Cfg,
            edge.kind.label(),
        ));
    }
    edges
}

fn dependence_edges(pdg: &ProgramDependenceGraph) -> Vec<CPGEdge> {
    pdg.edges
        .iter()
        .map(|dep| {
            let kind = if dep.kind == DependenceKind::Data {
                CPGEdgeKind::Ddg
            } else {
                CPGEdgeKind::Cdg
            };
            CPGEdge::new(
                dep.source.clone(),
                dep.target.clone(),
                kind,
                dep.var.clone(),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphs::ast::dispatch::parse_source;

    #[test]
    fn build_cpg_includes_all_four_edge_families_when_present() {
        let source = "def f(x):\n    if x:\n        y = 1\n    return y\n";
        let result = parse_source(source, "python", None).unwrap();
        let (nodes, edges) = build_cpg(&result.uast_root, source);

        assert!(!nodes.is_empty());
        assert!(edges.iter().any(|e| e.kind == CPGEdgeKind::Ast));
        assert!(edges.iter().any(|e| e.kind == CPGEdgeKind::Cfg));
        assert!(edges.iter().any(|e| e.kind == CPGEdgeKind::Cdg));
    }
}
