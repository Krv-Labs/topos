//! Semantic Compositional Divergence — the NAVIGABLE probe.
//!
//! # What this measures, and why it isn't cyclomatic complexity
//!
//! Classical complexity metrics stop predicting LLM task accuracy once
//! code length is controlled for; *nesting depth* keeps predicting it.
//! A flat function with six sequential `if`s and a six-deep nested one
//! have the same cyclomatic complexity, but models reason reliably over
//! the first and fail on the second — each nesting level is another
//! hierarchical state the attention mechanism has to hold open while it
//! reads forward.
//!
//! So NAVIGABLE measures nesting, and only nesting. For every
//! scope-forming node `u` inside a callable:
//!
//! ```text
//! SCD(fn) = Σ_u  depth(u) · ln(1 + fanout(u))
//! ```
//!
//! where `depth(u)` is `u`'s nesting level relative to the function body
//! (body = 0) and `fanout(u)` is `u`'s count of *immediate* child scopes.
//!
//! Two consequences of that shape are load-bearing, not accidents:
//!
//! - A leaf scope contributes `ln(1) = 0`. A perfectly flat function
//!   therefore scores exactly `0.0` — flat *is* maximally navigable, and
//!   a function's branch count is already SIMPLE's concern via
//!   `ast.max_function_complexity`. Deep code is still fully counted;
//!   the weight just lands on the ancestors that do the nesting.
//! - `ConditionalExpr` and short-circuit `BinaryExpr` are excluded.
//!   Expression-level branching opens no block, so it costs no state to
//!   track, and counting it here would just re-measure SIMPLE.
//!
//! # Gate granularity
//!
//! The gate is the per-function max, not a file-wide sum. This repo
//! already learned that lesson once: `cfg.cyclomatic` is a whole-file
//! merged-CFG sum that scales with function count, so it was demoted to
//! advisory (issue #193) and `ast.max_function_complexity` gates in its
//! place. A file-wide SCD sum would fail a long file full of short flat
//! functions for its length. The worst single function is the thing an
//! agent can actually act on.

use crate::functors::probes::ast::scopes::function_scopes;
use crate::graphs::uast::models::UASTNode;

/// UAST kinds that open a nested *block* scope.
///
/// Deliberately narrower than `complexity::DECISION_UAST_KINDS`: this is
/// about structural nesting, not decision counting. `FunctionDecl` /
/// `MethodDecl` appear because a nested closure is a real nesting level
/// for a reader — but note that a callable's own root node is never
/// counted against itself (see [`function_divergence`]).
const NAV_SCOPE_KINDS: &[&str] = &[
    "IfStmt",
    "ForStmt",
    "WhileStmt",
    "MatchStmt",
    "TryStmt",
    "WithStmt",
    "FunctionDecl",
    "MethodDecl",
];

fn is_scope(node: &UASTNode) -> bool {
    NAV_SCOPE_KINDS.contains(&node.kind.as_str())
}

/// Immediate child scopes of `node` in the *scope* tree — the nearest
/// scope-forming descendants, skipping straight through the intervening
/// block/statement plumbing that differs per grammar.
fn child_scopes(node: &UASTNode) -> Vec<&UASTNode> {
    let mut found = Vec::new();
    let mut stack: Vec<&UASTNode> = node.children.iter().collect();
    while let Some(candidate) = stack.pop() {
        if is_scope(candidate) {
            found.push(candidate);
        } else {
            stack.extend(candidate.children.iter());
        }
    }
    found
}

/// `SCD` for one callable's subtree.
///
/// The callable's own node is the body at depth 0, so it contributes
/// nothing itself — a function is not nested inside itself. Its child
/// scopes start at depth 1.
pub fn function_divergence(node: &UASTNode) -> f64 {
    fn walk(node: &UASTNode, depth: usize, total: &mut f64) {
        let children = child_scopes(node);
        if depth > 0 {
            *total += depth as f64 * (1.0 + children.len() as f64).ln();
        }
        for child in children {
            walk(child, depth + 1, total);
        }
    }
    let mut total = 0.0;
    walk(node, 0, &mut total);
    total
}

/// One callable's divergence, name, span, and scope kind.
///
/// Mirrors `complexity::FunctionComplexityEntry` — both are built from
/// the same [`function_scopes`] walk, so a failing gate always has a
/// location.
pub struct FunctionDivergenceEntry {
    pub name: String,
    pub qualified_name: String,
    pub kind: &'static str,
    pub start_line: usize,
    pub end_line: usize,
    pub divergence: f64,
}

/// Per-function divergence with locations, parallel to the gate metric.
pub fn calculate_function_divergence_entries(
    uast_root: &UASTNode,
    source: &str,
) -> Vec<FunctionDivergenceEntry> {
    function_scopes(uast_root, source)
        .into_iter()
        .map(|scope| FunctionDivergenceEntry {
            name: scope.name,
            qualified_name: scope.qualified_name,
            kind: scope.kind,
            start_line: scope.start_line,
            end_line: scope.end_line,
            divergence: function_divergence(scope.node),
        })
        .collect()
}

/// The `nav.max_function_divergence` gate metric.
///
/// `0.0` when the file declares no callables at all — a module of
/// constants is trivially navigable, and the metric is still emitted so
/// every file has a row in the calibration corpus.
pub fn calculate_max_function_divergence(uast_root: &UASTNode) -> f64 {
    let mut stack: Vec<&UASTNode> = vec![uast_root];
    let mut max = 0.0f64;
    while let Some(node) = stack.pop() {
        if matches!(node.kind.as_str(), "FunctionDecl" | "MethodDecl") {
            max = max.max(function_divergence(node));
        }
        stack.extend(node.children.iter());
    }
    max
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphs::ast::dispatch::parse_source;

    fn divergence(source: &str, language: &str) -> f64 {
        let result = parse_source(source, language, None).expect("parse should not fail");
        calculate_max_function_divergence(&result.uast_root)
    }

    /// The thesis of the whole pillar: sequential branching is free,
    /// nesting is not. Both functions have the same cyclomatic
    /// complexity, so SIMPLE cannot tell them apart — NAVIGABLE must.
    #[test]
    fn nesting_costs_where_sequential_branching_does_not() {
        let flat = "def f(a, b, c):\n    if a:\n        return 1\n    if b:\n        return 2\n    if c:\n        return 3\n    return 0\n";
        let nested =
            "def f(a, b, c):\n    if a:\n        if b:\n            if c:\n                return 3\n    return 0\n";
        assert_eq!(divergence(flat, "python"), 0.0);
        assert!(
            divergence(nested, "python") > divergence(flat, "python"),
            "nested must diverge more than flat"
        );
    }

    #[test]
    fn flat_function_is_zero() {
        assert_eq!(divergence("def f(x):\n    return x\n", "python"), 0.0);
    }

    #[test]
    fn no_functions_is_zero() {
        assert_eq!(divergence("x = 1\n", "python"), 0.0);
    }

    /// A chain of single-child scopes: depths 1, 2, 3 each with fanout 1,
    /// plus the innermost leaf at depth 4 with fanout 0.
    /// (1 + 2 + 3) * ln(2) = 4.159.
    #[test]
    fn four_deep_chain_matches_the_closed_form() {
        let source = "def f(a, b, c, d):\n    if a:\n        if b:\n            if c:\n                if d:\n                    return 1\n    return 0\n";
        let expected = 6.0 * 2.0f64.ln();
        assert!((divergence(source, "python") - expected).abs() < 1e-9);
    }

    #[test]
    fn deeper_nesting_diverges_more() {
        let two = "def f(a, b):\n    if a:\n        if b:\n            return 1\n    return 0\n";
        let three = "def f(a, b, c):\n    if a:\n        if b:\n            if c:\n                return 1\n    return 0\n";
        assert!(divergence(three, "python") > divergence(two, "python"));
    }

    #[test]
    fn loops_and_try_blocks_nest_too() {
        let source = "def f(items):\n    for item in items:\n        try:\n            handle(item)\n        except ValueError:\n            pass\n";
        assert!(divergence(source, "python") > 0.0);
    }

    /// A nested closure is a real nesting level for a reader, and its own
    /// body is measured independently too.
    #[test]
    fn nested_closure_counts_as_a_scope() {
        let source = "def outer(xs):\n    def inner(y):\n        if y:\n            return 1\n        return 0\n    return [inner(x) for x in xs]\n";
        assert!(divergence(source, "python") > 0.0);
    }

    /// Sibling scopes at the same level raise fanout on their parent, so
    /// a wide nested block costs more than a narrow one at equal depth.
    #[test]
    fn fanout_raises_divergence_at_equal_depth() {
        let narrow = "def f(a, b):\n    if a:\n        if b:\n            return 1\n    return 0\n";
        let wide = "def f(a, b, c):\n    if a:\n        if b:\n            return 1\n        if c:\n            return 2\n    return 0\n";
        assert!(divergence(wide, "python") > divergence(narrow, "python"));
    }

    /// Expression-level branching is SIMPLE's business, not NAVIGABLE's:
    /// a ternary opens no block and costs no reader state.
    #[test]
    fn expression_branching_is_not_nesting() {
        assert_eq!(
            divergence("function f(x) { return x ? 1 : 0; }\n", "javascript"),
            0.0
        );
    }

    #[test]
    fn works_across_every_supported_language() {
        let cases: &[(&str, &str, &str)] = &[
            (
                "python",
                "def f(x):\n    return x\n",
                "def f(a, b):\n    if a:\n        if b:\n            return 1\n    return 0\n",
            ),
            (
                "rust",
                "fn f(x: i32) -> i32 {\n    x\n}\n",
                "fn f(a: bool, b: bool) -> i32 {\n    if a {\n        if b {\n            return 1;\n        }\n    }\n    0\n}\n",
            ),
            (
                "javascript",
                "function f(x) {\n  return x;\n}\n",
                "function f(a, b) {\n  if (a) {\n    if (b) {\n      return 1;\n    }\n  }\n  return 0;\n}\n",
            ),
            (
                "typescript",
                "function f(x: number): number {\n  return x;\n}\n",
                "function f(a: boolean, b: boolean): number {\n  if (a) {\n    if (b) {\n      return 1;\n    }\n  }\n  return 0;\n}\n",
            ),
            (
                "go",
                "package p\nfunc f(x int) int {\n\treturn x\n}\n",
                "package p\nfunc f(a bool, b bool) int {\n\tif a {\n\t\tif b {\n\t\t\treturn 1\n\t\t}\n\t}\n\treturn 0\n}\n",
            ),
            (
                "cpp",
                "int f(int x) {\n  return x;\n}\n",
                "int f(bool a, bool b) {\n  if (a) {\n    if (b) {\n      return 1;\n    }\n  }\n  return 0;\n}\n",
            ),
        ];

        for (language, flat, nested) in cases {
            assert_eq!(
                divergence(flat, language),
                0.0,
                "flat {language} function must be 0.0"
            );
            assert!(
                divergence(nested, language) > 0.0,
                "nested {language} function must diverge: {nested:?}"
            );
        }
    }

    /// The regression that matters, mirroring
    /// `complexity::every_gate_counted_function_has_exactly_one_entry`:
    /// if the gate metric and the location path disagree, a real failure
    /// has no location, never becomes a refactor target, and can never be
    /// fixed. Both paths must come from the same walk.
    #[test]
    fn worst_entry_matches_the_gate_metric() {
        let cases: &[(&str, &str)] = &[
            (
                "python",
                "class C:\n    def m(self, a, b):\n        if a:\n            if b:\n                return 1\n        return 0\n",
            ),
            (
                "rust",
                "pub fn f(a: bool, b: bool) -> i32 {\n    if a {\n        if b {\n            return 1;\n        }\n    }\n    0\n}\n",
            ),
            (
                "javascript",
                "[1, 2].map(function (x) {\n  if (x) {\n    if (x > 1) {\n      return 1;\n    }\n  }\n  return 0;\n});\n",
            ),
            ("python", "x = 1\n"),
        ];

        for (language, source) in cases {
            let result = parse_source(source, language, None).expect("parse should not fail");
            let entries = calculate_function_divergence_entries(&result.uast_root, source);
            let worst = entries
                .iter()
                .map(|e| e.divergence)
                .fold(0.0f64, |acc, d| acc.max(d));
            assert!(
                (worst - calculate_max_function_divergence(&result.uast_root)).abs() < 1e-9,
                "worst entry must match the gate metric for {language}: {source:?}"
            );
        }
    }

    #[test]
    fn entries_carry_names_and_spans() {
        let source = "def outer(a, b):\n    if a:\n        if b:\n            return 1\n    return 0\n";
        let result = parse_source(source, "python", None).unwrap();
        let entries = calculate_function_divergence_entries(&result.uast_root, source);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].qualified_name, "outer");
        assert_eq!(entries[0].start_line, 1);
        assert!(entries[0].divergence > 0.0);
    }
}
