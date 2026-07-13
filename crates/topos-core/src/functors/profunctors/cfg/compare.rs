//! CFG Comparison — profunctor `D : E × E^op → ℝ` restricted to CFGs.
//!
//! Pairwise comparison of two [`ControlFlowGraph`] instances. The CFG
//! captures intra-procedural control flow; comparing two CFGs lets us
//! reason about how a refactor changed branching shape *without* being
//! misled by lexical churn (whitespace, renames) the AST distance picks
//! up.
//!
//! Three orthogonal signals are exposed:
//!
//! - `cyclomatic_delta` — signed change in McCabe complexity
//! - `edge_kind_l1` — L1 distance between edge-kind histograms (a
//!   single number in `[0, 2]`)
//! - `longest_path_delta` — signed change in longest acyclic
//!   entry→exit path
//!
//! A composite [`CFGComparison`] packages all three plus the raw
//! per-side measurements so callers can render whatever summary they
//! need.

use std::collections::{HashMap, HashSet};

use crate::graphs::base::Representation;
use crate::graphs::cfg::object::ControlFlowGraph;

/// Pairwise comparison summary for two control-flow graphs.
#[derive(Debug, Clone, PartialEq)]
pub struct CFGComparison {
    pub cyclomatic_delta: i64,
    pub edge_kind_l1: f64,
    pub longest_path_delta: i64,
    pub source_metrics: HashMap<String, f64>,
    pub target_metrics: HashMap<String, f64>,
}

impl CFGComparison {
    /// True iff any signal reports a non-zero divergence.
    pub fn changed(&self) -> bool {
        self.cyclomatic_delta != 0 || self.edge_kind_l1 > 0.0 || self.longest_path_delta != 0
    }
}

/// Signed change in McCabe cyclomatic complexity (target − source).
pub fn cyclomatic_delta(source: &ControlFlowGraph, target: &ControlFlowGraph) -> i64 {
    target.cyclomatic_complexity() as i64 - source.cyclomatic_complexity() as i64
}

/// Count edges grouped by [`crate::graphs::cfg::models::EdgeKind`].
pub fn edge_kind_histogram(cfg: &ControlFlowGraph) -> HashMap<String, usize> {
    let mut histogram = HashMap::new();
    for edge in &cfg.edges {
        *histogram.entry(edge.kind.label().to_string()).or_insert(0) += 1;
    }
    histogram
}

/// L1 distance between the two edge-kind histograms, normalized to
/// probability distributions. Result lies in `[0, 1]` (half of the raw
/// L1, matching the convention used elsewhere in the codebase).
pub fn edge_kind_l1_distance(source: &ControlFlowGraph, target: &ControlFlowGraph) -> f64 {
    let a = edge_kind_histogram(source);
    let b = edge_kind_histogram(target);
    let total_a = a.values().sum::<usize>().max(1) as f64;
    let total_b = b.values().sum::<usize>().max(1) as f64;

    let kinds: HashSet<&String> = a.keys().chain(b.keys()).collect();
    let l1: f64 = kinds
        .iter()
        .map(|k| {
            let av = *a.get(*k).unwrap_or(&0) as f64 / total_a;
            let bv = *b.get(*k).unwrap_or(&0) as f64 / total_b;
            (av - bv).abs()
        })
        .sum();
    l1 / 2.0
}

/// Signed change in longest acyclic entry→exit path length.
pub fn longest_path_delta(source: &ControlFlowGraph, target: &ControlFlowGraph) -> i64 {
    target.longest_acyclic_path() as i64 - source.longest_acyclic_path() as i64
}

/// Run the full CFG comparison suite for a single pair of graphs.
pub fn compare_cfg(source: &ControlFlowGraph, target: &ControlFlowGraph) -> CFGComparison {
    CFGComparison {
        cyclomatic_delta: cyclomatic_delta(source, target),
        edge_kind_l1: edge_kind_l1_distance(source, target),
        longest_path_delta: longest_path_delta(source, target),
        source_metrics: source.metrics(),
        target_metrics: target.metrics(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::morphism::ProgramMorphism;

    fn cfg_of(source: &str) -> ControlFlowGraph {
        let mut morphism = ProgramMorphism::new(source, "python");
        morphism.build_cfg().unwrap().clone()
    }

    #[test]
    fn cfg_compare_identical_is_zero() {
        let src = "def f(x):\n    if x: return 1\n    return 0\n";
        let a = cfg_of(src);
        let b = cfg_of(src);
        let cmp = compare_cfg(&a, &b);
        assert_eq!(cmp.cyclomatic_delta, 0);
        assert_eq!(cmp.edge_kind_l1, 0.0);
        assert_eq!(cmp.longest_path_delta, 0);
        assert!(!cmp.changed());
    }

    #[test]
    fn cfg_compare_detects_added_branch() {
        let simpler = "def f(x):\n    return 0\n";
        let branchy = "def f(x):\n    if x > 0:\n        return 1\n    return 0\n";
        let a = cfg_of(simpler);
        let b = cfg_of(branchy);
        assert!(cyclomatic_delta(&a, &b) > 0);
        let cmp = compare_cfg(&a, &b);
        assert!(cmp.changed());
        assert_eq!(cmp.cyclomatic_delta, cyclomatic_delta(&a, &b));
    }
}
