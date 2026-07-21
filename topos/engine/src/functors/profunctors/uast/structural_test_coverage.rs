//! Structural test coverage (UAST).
//!
//! PUT-directed recall: how much of the program-under-test's UAST
//! structure is represented in the test suite's UAST, using kind
//! histograms, control-flow profiles, and optional k-gram path overlap.

use std::collections::HashMap;
use std::fmt;

use crate::functors::probes::uast::signature::{
    control_flow_profile, uast_dfs_kind_sequence, uast_kind_histogram, CONTROL_FLOW_KINDS,
};
use crate::graphs::uast::models::UASTNode;

const STMT_KINDS: &[&str] = &[
    "IfStmt",
    "ForStmt",
    "WhileStmt",
    "MatchStmt",
    "ReturnStmt",
    "BreakStmt",
    "ContinueStmt",
    "ThrowStmt",
    "TryStmt",
    "ExprStmt",
];

const EXPR_KINDS: &[&str] = &[
    "AssignExpr",
    "BinaryExpr",
    "UnaryExpr",
    "CallExpr",
    "MemberExpr",
];

const DECL_KINDS: &[&str] = &["FunctionDecl", "MethodDecl"];

/// Raised by [`declaration_coverage`] when `k < 1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidKError(pub usize);

impl fmt::Display for InvalidKError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "k must be >= 1, got {}", self.0)
    }
}

impl std::error::Error for InvalidKError {}

/// Sum [`uast_kind_histogram`] across multiple UAST roots.
pub fn merge_uast_kind_histograms(
    roots: &[&UASTNode],
    include_unknown: bool,
) -> HashMap<String, usize> {
    let mut merged: HashMap<String, usize> = HashMap::new();
    for root in roots {
        for (kind, count) in uast_kind_histogram(root, include_unknown) {
            *merged.entry(kind).or_insert(0) += count;
        }
    }
    merged
}

/// Sum [`control_flow_profile`] across multiple UAST roots.
pub fn merge_control_flow_profiles(roots: &[&UASTNode]) -> HashMap<String, usize> {
    let mut merged: HashMap<String, usize> = CONTROL_FLOW_KINDS
        .iter()
        .map(|k| (k.to_string(), 0))
        .collect();
    for root in roots {
        let profile = control_flow_profile(root);
        for kind in CONTROL_FLOW_KINDS {
            *merged.get_mut(*kind).unwrap() += profile.get(*kind).copied().unwrap_or(0);
        }
    }
    merged
}

/// `sum_k min(n_P(k), n_T(k)) / sum_k n_P(k)`.
///
/// Vacuous denominator (empty PUT multiset) yields `1.0`.
fn multiset_recall(
    counts_put: &HashMap<String, usize>,
    counts_test: &HashMap<String, usize>,
) -> f64 {
    let denom: usize = counts_put.values().sum();
    if denom == 0 {
        return 1.0;
    }
    let num: usize = counts_put
        .iter()
        .map(|(k, &v)| v.min(counts_test.get(k).copied().unwrap_or(0)))
        .sum();
    num as f64 / denom as f64
}

fn kgrams_from_sequence(seq: &[String], k: usize) -> HashMap<Vec<String>, usize> {
    let mut counts: HashMap<Vec<String>, usize> = HashMap::new();
    if k < 1 || seq.len() < k {
        return counts;
    }
    for window in seq.windows(k) {
        *counts.entry(window.to_vec()).or_insert(0) += 1;
    }
    counts
}

/// Multiset of length-`k` kind n-grams, aggregated per root then summed.
///
/// Each root is DFS-sequenced independently; k-grams never span file
/// boundaries.
pub fn merge_kgram_counters(
    roots: &[&UASTNode],
    k: usize,
    include_unknown: bool,
) -> HashMap<Vec<String>, usize> {
    let mut total: HashMap<Vec<String>, usize> = HashMap::new();
    for root in roots {
        let seq = uast_dfs_kind_sequence(root, include_unknown);
        for (gram, count) in kgrams_from_sequence(&seq, k) {
            *total.entry(gram).or_insert(0) += count;
        }
    }
    total
}

fn kgram_recall(c_put: &HashMap<Vec<String>, usize>, c_test: &HashMap<Vec<String>, usize>) -> f64 {
    let denom: usize = c_put.values().sum();
    if denom == 0 {
        return 1.0;
    }
    let num: usize = c_put
        .iter()
        .map(|(g, &v)| v.min(c_test.get(g).copied().unwrap_or(0)))
        .sum();
    num as f64 / denom as f64
}

// Declaration-level bipartite coverage

/// Return all FunctionDecl/MethodDecl UASTNodes via DFS (includes
/// nested).
pub fn extract_declarations(root: &UASTNode) -> Vec<&UASTNode> {
    let mut results = Vec::new();
    let mut stack: Vec<&UASTNode> = vec![root];
    while let Some(node) = stack.pop() {
        if DECL_KINDS.contains(&node.kind.as_str()) {
            results.push(node);
        }
        stack.extend(node.children.iter().rev());
    }
    results
}

/// Kind histogram of a declaration subtree with the root kind removed.
///
/// Stripping the root kind prevents every PUT/test pair from getting a
/// free floor score from the shared `FunctionDecl`/`MethodDecl` node.
fn decl_fingerprint(decl_node: &UASTNode, include_unknown: bool) -> HashMap<String, usize> {
    let mut hist = uast_kind_histogram(decl_node, include_unknown);
    if let Some(count) = hist.get_mut(&decl_node.kind) {
        if *count <= 1 {
            hist.remove(&decl_node.kind);
        } else {
            *count -= 1;
        }
    }
    hist
}

fn location_str(node: &UASTNode) -> String {
    match &node.span.file {
        Some(file) if !file.is_empty() => format!("{file}:{}", node.span.start_line),
        _ => format!("line:{}", node.span.start_line),
    }
}

/// Declaration-level bipartite structural coverage.
///
/// Each `FunctionDecl`/`MethodDecl` in the PUT is matched against the
/// test suite's declarations via greedy best-match recall. Scores are
/// not inflated by adding unrelated test code (not monotone with
/// corpus size).
#[derive(Debug, Clone, PartialEq)]
pub struct DeclarationCoverageReport {
    pub mean_declaration_coverage: f64,
    pub best_declaration_recall: Vec<f64>,
    pub declaration_locations: Vec<String>,
    pub stmt_recall: f64,
    pub expr_recall: f64,
    pub mean_test_precision: f64,
    pub declaration_path_recall_kgram: f64,
    pub k: usize,
    pub put_declaration_count: usize,
    pub test_declaration_count: usize,
    pub include_unknown: bool,
}

impl DeclarationCoverageReport {
    /// Alias for `mean_declaration_coverage` (backward compatibility).
    pub fn declaration_coverage_rate(&self) -> f64 {
        self.mean_declaration_coverage
    }

    /// F2 score favoring recall over precision.
    pub fn f2_score(&self) -> f64 {
        let p = self.mean_test_precision;
        let r = self.mean_declaration_coverage;
        if p + r == 0.0 {
            return 0.0;
        }
        5.0 * (p * r) / (4.0 * p + r)
    }

    /// Locations of PUT declarations with incomplete test coverage.
    pub fn uncovered_declarations(&self) -> Vec<String> {
        self.declaration_locations
            .iter()
            .zip(&self.best_declaration_recall)
            .filter(|(_, &recall)| recall < 0.999) // precision-safe 1.0
            .map(|(loc, _)| loc.clone())
            .collect()
    }
}

fn filter_kinds(histogram: &HashMap<String, usize>, kinds: &[&str]) -> HashMap<String, usize> {
    histogram
        .iter()
        .filter(|(k, _)| kinds.contains(&k.as_str()))
        .map(|(k, &v)| (k.clone(), v))
        .collect()
}

/// Declaration-level bipartite structural coverage.
///
/// For each `FunctionDecl`/`MethodDecl` in the PUT, finds the
/// best-matching test declaration by multiset recall of body kind
/// histograms. Addresses the five weaknesses of earlier metrics:
///
/// - Pooled histograms replaced by per-declaration matching (localizable gaps)
/// - CF/kind double-counting replaced by disjoint stmt/expr category recall
/// - DFS k-grams scoped to declaration subtrees (semantically bounded)
/// - Not monotone: unrelated test functions do not inflate PUT coverage
/// - Precision signal added: F2 score penalizes bloated test suites
///
/// Vacuous PUT (no declarations) yields `1.0` for all recall scores
/// with `put_declaration_count = 0`.
pub fn declaration_coverage(
    put_roots: &[&UASTNode],
    test_roots: &[&UASTNode],
    k: usize,
    include_unknown: bool,
) -> Result<DeclarationCoverageReport, InvalidKError> {
    if k < 1 {
        return Err(InvalidKError(k));
    }

    let put_decls: Vec<&UASTNode> = put_roots
        .iter()
        .flat_map(|r| extract_declarations(r))
        .collect();
    let test_decls: Vec<&UASTNode> = test_roots
        .iter()
        .flat_map(|r| extract_declarations(r))
        .collect();

    let put_fps: Vec<HashMap<String, usize>> = put_decls
        .iter()
        .map(|d| decl_fingerprint(d, include_unknown))
        .collect();
    let test_fps: Vec<HashMap<String, usize>> = test_decls
        .iter()
        .map(|d| decl_fingerprint(d, include_unknown))
        .collect();

    let best_recall: Vec<f64> = put_fps
        .iter()
        .map(|pf| {
            test_fps
                .iter()
                .map(|tf| multiset_recall(pf, tf))
                .fold(0.0_f64, f64::max)
        })
        .collect();

    let mean_decl_cov = if !put_decls.is_empty() {
        best_recall.iter().sum::<f64>() / best_recall.len() as f64
    } else {
        1.0
    };

    // Category-stratified recall — disjoint Stmt vs Expr subsets, no
    // double-counting.
    let h_put = merge_uast_kind_histograms(put_roots, include_unknown);
    let h_test = merge_uast_kind_histograms(test_roots, include_unknown);
    let stmt_recall = multiset_recall(
        &filter_kinds(&h_put, STMT_KINDS),
        &filter_kinds(&h_test, STMT_KINDS),
    );
    let expr_recall = multiset_recall(
        &filter_kinds(&h_put, EXPR_KINDS),
        &filter_kinds(&h_test, EXPR_KINDS),
    );

    let best_prec: Vec<f64> = test_fps
        .iter()
        .map(|tf| {
            put_fps
                .iter()
                .map(|pf| multiset_recall(tf, pf))
                .fold(0.0_f64, f64::max)
        })
        .collect();
    let mean_test_prec = if !test_decls.is_empty() {
        best_prec.iter().sum::<f64>() / best_prec.len() as f64
    } else {
        0.0
    };

    let put_kg = merge_kgram_counters(&put_decls, k, include_unknown);
    let test_kg = merge_kgram_counters(&test_decls, k, include_unknown);

    Ok(DeclarationCoverageReport {
        mean_declaration_coverage: mean_decl_cov,
        best_declaration_recall: best_recall,
        declaration_locations: put_decls.iter().map(|d| location_str(d)).collect(),
        stmt_recall,
        expr_recall,
        mean_test_precision: mean_test_prec,
        declaration_path_recall_kgram: kgram_recall(&put_kg, &test_kg),
        k,
        put_declaration_count: put_decls.len(),
        test_declaration_count: test_decls.len(),
        include_unknown,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphs::ast::dispatch::parse_source;

    fn uast(source: &str) -> UASTNode {
        parse_source(source, "python", None).unwrap().uast_root
    }

    #[test]
    fn merge_histograms_two_roots() {
        let a = uast("def a(): pass\n");
        let b = uast("def b(): return 1\n");
        let merged = merge_uast_kind_histograms(&[&a, &b], false);
        let single = merge_uast_kind_histograms(&[&a], false);
        assert!(merged.values().sum::<usize>() > single.values().sum::<usize>());
    }

    #[test]
    fn decl_coverage_identical_put_and_test_full_scores() {
        let root = uast(
            "def f(n):\n    for i in range(n):\n        if i % 2:\n            pass\n    return n\n",
        );
        let rep = declaration_coverage(&[&root], &[&root], 3, false).unwrap();
        assert!((rep.mean_declaration_coverage - 1.0).abs() < 1e-9);
        assert_eq!(rep.best_declaration_recall.len(), 1);
        assert!((rep.best_declaration_recall[0] - 1.0).abs() < 1e-9);
        assert!((rep.stmt_recall - 1.0).abs() < 1e-9);
        assert!((rep.declaration_path_recall_kgram - 1.0).abs() < 1e-9);
    }

    #[test]
    fn decl_coverage_empty_tests_yield_zero() {
        let put =
            uast("def g():\n    while True:\n        if True:\n            break\n    return 0\n");
        let rep = declaration_coverage(&[&put], &[], 3, true).unwrap();
        assert_eq!(rep.mean_declaration_coverage, 0.0);
        assert_eq!(rep.best_declaration_recall, vec![0.0]);
        assert_eq!(rep.mean_test_precision, 0.0);
    }

    #[test]
    fn decl_coverage_unrelated_test_does_not_inflate() {
        let put = uast(
            "def process(xs):\n    result = []\n    for x in xs:\n        if x > 0:\n            result.append(x)\n    return result\n",
        );
        let unrelated_test =
            uast("def test_math():\n    a = 1 + 2\n    b = a * 3\n    assert b == 9\n");
        let rep = declaration_coverage(&[&put], &[&unrelated_test], 3, true).unwrap();
        assert!(rep.mean_declaration_coverage < 1.0);
    }

    #[test]
    fn decl_coverage_bloated_test_does_not_fully_cover_focused_put() {
        let put = uast(
            "def compute(data):\n    total = 0\n    for item in data:\n        if item > 0:\n            total += item\n        elif item < 0:\n            total -= item\n    return total\n",
        );
        let bloated = uast(
            "def test_a(): pass\ndef test_b(): pass\ndef test_c(): pass\ndef test_d(): pass\ndef test_e(): pass\n",
        );
        let rep_bloated = declaration_coverage(&[&put], &[&bloated], 3, true).unwrap();
        let tight_test = uast(
            "def test_compute():\n    total = 0\n    for item in [1, -2, 3]:\n        if item > 0:\n            total += item\n        elif item < 0:\n            total -= item\n    assert total == 2\n",
        );
        let rep_tight = declaration_coverage(&[&put], &[&tight_test], 3, true).unwrap();
        assert!(rep_tight.mean_declaration_coverage > rep_bloated.mean_declaration_coverage);
    }

    #[test]
    fn decl_coverage_precision_tight_vs_bloated() {
        let put = uast("def f(x):\n    if x > 0:\n        return x\n    return -x\n");
        let tight = uast("def test_f():\n    if True:\n        return 1\n    return -1\n");
        let bloated = uast(
            "def test_a():\n    x = 1 + 2 + 3 + 4 + 5\n    y = x * x * x\n    return y\ndef test_b():\n    items = [1, 2, 3]\n    total = items[0] + items[1] + items[2]\n    return total\ndef test_f():\n    if True:\n        return 1\n    return -1\n",
        );
        let rep_tight = declaration_coverage(&[&put], &[&tight], 3, true).unwrap();
        let rep_bloated = declaration_coverage(&[&put], &[&bloated], 3, true).unwrap();
        assert!(rep_tight.mean_test_precision > rep_bloated.mean_test_precision);
    }

    #[test]
    fn decl_coverage_category_stratified_disjoint() {
        let put = uast(
            "def f():\n    for i in range(3):\n        if i > 0:\n            continue\n    return None\n",
        );
        let test_src = uast(
            "def t():\n    for i in range(3):\n        if i > 0:\n            continue\n    return None\n",
        );
        let rep = declaration_coverage(&[&put], &[&test_src], 3, true).unwrap();
        assert!(rep.stmt_recall > 0.0);
        assert!((0.0..=1.0).contains(&rep.stmt_recall));
        assert!((0.0..=1.0).contains(&rep.expr_recall));
    }

    #[test]
    fn extract_declarations_finds_function_and_method() {
        let root = uast("class MyClass:\n    def method(self):\n        pass\n\ndef standalone():\n    return 1\n");
        let decls = extract_declarations(&root);
        assert!(decls.len() >= 2);
        for d in &decls {
            assert!(d.kind == "FunctionDecl" || d.kind == "MethodDecl");
        }
    }

    #[test]
    fn decl_coverage_vacuous_empty_put() {
        let put = uast("x = 1\ny = x + 2\n");
        let test_src = uast("def test_something(): pass\n");
        let rep = declaration_coverage(&[&put], &[&test_src], 3, true).unwrap();
        assert_eq!(rep.put_declaration_count, 0);
        assert!((rep.mean_declaration_coverage - 1.0).abs() < 1e-9);
    }

    #[test]
    fn decl_coverage_invalid_k_raises() {
        let put = uast("def f(): return 0\n");
        let err = declaration_coverage(&[&put], &[&put], 0, true).unwrap_err();
        assert!(err.to_string().contains("k must be"));
    }
}
