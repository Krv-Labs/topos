//! UAST Signature — cheap, language-agnostic structural fingerprints of
//! a UAST root.
//!
//! These signatures are designed to summarize a program's shape across
//! languages so that two implementations of the same algorithm can be
//! compared without depending on language-specific node names.

use std::collections::HashMap;

use crate::graphs::uast::models::UASTNode;

/// UAST kinds considered control-flow-relevant (loops, branches, calls,
/// returns).
pub const CONTROL_FLOW_KINDS: &[&str] = &[
    "IfStmt",
    "ForStmt",
    "WhileStmt",
    "MatchStmt",
    "TryStmt",
    "ReturnStmt",
    "BreakStmt",
    "ContinueStmt",
    "ThrowStmt",
    "CallExpr",
];

/// Aggregate, language-agnostic stats about a UAST.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StructuralSummary {
    pub node_count: usize,
    pub depth: usize,
    pub declaration_count: usize,
    pub expression_count: usize,
    pub statement_count: usize,
}

fn walk<'a>(root: &'a UASTNode, out: &mut Vec<&'a UASTNode>) {
    out.push(root);
    for child in &root.children {
        walk(child, out);
    }
}

/// Count UAST `kind` occurrences across the whole tree.
///
/// When `include_unknown` is `false`, drops the catch-all `Unknown`
/// bucket so grammar-coverage gaps don't dominate the histogram for
/// languages with thinner UAST mapping coverage.
pub fn uast_kind_histogram(root: &UASTNode, include_unknown: bool) -> HashMap<String, usize> {
    let mut nodes = Vec::new();
    walk(root, &mut nodes);
    let mut counts: HashMap<String, usize> = HashMap::new();
    for node in nodes {
        if !include_unknown && node.kind == "Unknown" {
            continue;
        }
        *counts.entry(node.kind.clone()).or_insert(0) += 1;
    }
    counts
}

/// DFS pre-order traversal of UAST `kind` strings (same order as edit
/// distance).
///
/// When `include_unknown` is `false`, nodes mapped to `Unknown` are
/// omitted from the sequence entirely (not counted as a step).
pub fn uast_dfs_kind_sequence(root: &UASTNode, include_unknown: bool) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack: Vec<&UASTNode> = vec![root];
    while let Some(node) = stack.pop() {
        if include_unknown || node.kind != "Unknown" {
            out.push(node.kind.clone());
        }
        stack.extend(node.children.iter().rev());
    }
    out
}

/// Count control-flow-relevant UAST kinds (loops, branches, calls,
/// returns).
pub fn control_flow_profile(root: &UASTNode) -> HashMap<String, usize> {
    let mut profile: HashMap<String, usize> = CONTROL_FLOW_KINDS
        .iter()
        .map(|k| (k.to_string(), 0))
        .collect();
    let mut nodes = Vec::new();
    walk(root, &mut nodes);
    for node in nodes {
        if let Some(count) = profile.get_mut(node.kind.as_str()) {
            *count += 1;
        }
    }
    profile
}

/// Single-pass aggregate stats about the UAST.
pub fn structural_summary(root: &UASTNode) -> StructuralSummary {
    #[allow(clippy::too_many_arguments)]
    fn walk_depth(
        node: &UASTNode,
        depth: usize,
        node_count: &mut usize,
        declaration_count: &mut usize,
        expression_count: &mut usize,
        statement_count: &mut usize,
    ) -> usize {
        *node_count += 1;
        if node.kind.ends_with("Decl") {
            *declaration_count += 1;
        } else if node.kind.ends_with("Expr") {
            *expression_count += 1;
        } else if node.kind.ends_with("Stmt") {
            *statement_count += 1;
        }
        let mut max_depth = depth;
        for child in &node.children {
            max_depth = max_depth.max(walk_depth(
                child,
                depth + 1,
                node_count,
                declaration_count,
                expression_count,
                statement_count,
            ));
        }
        max_depth
    }

    let mut node_count = 0;
    let mut declaration_count = 0;
    let mut expression_count = 0;
    let mut statement_count = 0;
    let depth = walk_depth(
        root,
        0,
        &mut node_count,
        &mut declaration_count,
        &mut expression_count,
        &mut statement_count,
    );

    StructuralSummary {
        node_count,
        depth,
        declaration_count,
        expression_count,
        statement_count,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphs::ast::dispatch::parse_source;

    fn uast(source: &str, language: &str) -> UASTNode {
        parse_source(source, language, None).unwrap().uast_root
    }

    #[test]
    fn kind_histogram_excludes_unknown_when_requested() {
        let root = uast(
            "fn main() { let x = 1; if x > 0 { println!(\"ok\"); } }",
            "rust",
        );

        let with_unknown = uast_kind_histogram(&root, true);
        let without_unknown = uast_kind_histogram(&root, false);

        assert!(*with_unknown.get("Unknown").unwrap_or(&0) > 0);
        assert!(!without_unknown.contains_key("Unknown"));
        assert!(without_unknown.values().sum::<usize>() < with_unknown.values().sum::<usize>());
    }

    #[test]
    fn control_flow_profile_counts_loops_and_returns() {
        let root = uast(
            "def f(xs):\n    total = 0\n    for x in xs:\n        if x > 0:\n            total += x\n    return total\n",
            "python",
        );

        let profile = control_flow_profile(&root);
        assert_eq!(profile["ForStmt"], 1);
        assert_eq!(profile["IfStmt"], 1);
        assert_eq!(profile["ReturnStmt"], 1);
    }

    #[test]
    fn structural_summary_counts_declarations() {
        let root = uast(
            "def a(): pass\ndef b(): pass\nclass C:\n    def m(self): pass\n",
            "python",
        );

        let summary = structural_summary(&root);
        assert!(summary.node_count > 0);
        assert!(summary.depth > 0);
        assert!(summary.declaration_count >= 3); // two functions + one class
    }

    #[test]
    fn dfs_kind_sequence_is_preorder() {
        let root = uast("x = 1 + 2\n", "python");
        let seq = uast_dfs_kind_sequence(&root, true);
        assert_eq!(seq.first().map(String::as_str), Some(root.kind.as_str()));
    }
}
