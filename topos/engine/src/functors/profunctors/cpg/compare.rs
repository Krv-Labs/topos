//! CPG Comparison — profunctor `D : E × E^op → ℝ` restricted to the
//! Code Property Graph.
//!
//! Pairwise comparison of two [`CodePropertyGraph`] instances. The CPG
//! fuses AST ∪ CFG ∪ DDG ∪ CDG into a single labeled multigraph;
//! comparing two CPGs gives a single end-to-end signal for "did this
//! refactor change the program's semantic structure?".
//!
//! Signals:
//!
//! - `family_jaccards` — Jaccard similarity per edge family (`{AST,
//!   CFG, DDG, CDG} → float in [0, 1]`)
//! - `node_jaccard` — Jaccard similarity over CPG node ids
//! - `dangerous_delta` — signed change in count of dangerous-API call
//!   sites
//! - `taint_delta` — signed change in count of source → sink taint
//!   paths

use std::collections::{HashMap, HashSet};

use crate::graphs::base::Representation;
use crate::graphs::cpg::models::CPGEdgeKind;
use crate::graphs::cpg::object::CodePropertyGraph;

/// Pairwise comparison summary for two code-property graphs.
#[derive(Debug, Clone, PartialEq)]
pub struct CPGComparison {
    pub family_jaccards: HashMap<String, f64>,
    pub node_jaccard: f64,
    pub dangerous_delta: f64,
    pub taint_delta: f64,
    pub source_metrics: HashMap<String, f64>,
    pub target_metrics: HashMap<String, f64>,
}

impl CPGComparison {
    pub fn changed(&self) -> bool {
        if self.node_jaccard < 1.0 {
            return true;
        }
        if self.family_jaccards.values().any(|&v| v < 1.0) {
            return true;
        }
        self.dangerous_delta != 0.0 || self.taint_delta != 0.0
    }
}

fn jaccard<T: Eq + std::hash::Hash>(a: &HashSet<T>, b: &HashSet<T>) -> f64 {
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    let intersection = a.intersection(b).count();
    let union = a.union(b).count();
    intersection as f64 / union as f64
}

/// Jaccard similarity over CPG node ids (stable UAST node hashes).
pub fn node_jaccard(source: &CodePropertyGraph, target: &CodePropertyGraph) -> f64 {
    let a: HashSet<&String> = source.nodes.keys().collect();
    let b: HashSet<&String> = target.nodes.keys().collect();
    jaccard(&a, &b)
}

/// Jaccard similarity *per edge family* over edge identities.
///
/// Each edge is identified by the triple `(source, target, label)` so
/// that, e.g., two DDG edges over the same variable count as the same
/// edge and two CFG edges with different branch labels (`true` vs
/// `false`) count as distinct. Result keys are the [`CPGEdgeKind`]
/// labels (`ast`, `cfg`, `ddg`, `cdg`).
pub fn family_jaccards(
    source: &CodePropertyGraph,
    target: &CodePropertyGraph,
) -> HashMap<String, f64> {
    let mut out = HashMap::new();
    for kind in CPGEdgeKind::ALL {
        let a: HashSet<(&str, &str, &str)> = source
            .edges
            .iter()
            .filter(|e| e.kind == kind)
            .map(|e| (e.source.as_str(), e.target.as_str(), e.label.as_str()))
            .collect();
        let b: HashSet<(&str, &str, &str)> = target
            .edges
            .iter()
            .filter(|e| e.kind == kind)
            .map(|e| (e.source.as_str(), e.target.as_str(), e.label.as_str()))
            .collect();
        out.insert(kind.label().to_string(), jaccard(&a, &b));
    }
    out
}

/// Signed change in dangerous-API call-site count (target − source).
pub fn dangerous_delta(source: &CodePropertyGraph, target: &CodePropertyGraph) -> f64 {
    target.metrics()["cpg.dangerous_calls"] - source.metrics()["cpg.dangerous_calls"]
}

/// Signed change in source → sink taint-path count (target − source).
pub fn taint_delta(source: &CodePropertyGraph, target: &CodePropertyGraph) -> f64 {
    target.metrics()["cpg.taint_flows"] - source.metrics()["cpg.taint_flows"]
}

/// Run the full CPG comparison suite for a single pair of graphs.
pub fn compare_cpg(source: &CodePropertyGraph, target: &CodePropertyGraph) -> CPGComparison {
    CPGComparison {
        family_jaccards: family_jaccards(source, target),
        node_jaccard: node_jaccard(source, target),
        dangerous_delta: dangerous_delta(source, target),
        taint_delta: taint_delta(source, target),
        source_metrics: source.metrics(),
        target_metrics: target.metrics(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::morphism::ProgramMorphism;

    fn cpg_of(source: &str, language: &str) -> CodePropertyGraph {
        let mut morphism = ProgramMorphism::new(source, language);
        morphism.build_cpg().unwrap().clone()
    }

    #[test]
    fn cpg_compare_identical_is_full_jaccard() {
        let src = "def f(x):\n    return x + 1\n";
        let a = cpg_of(src, "python");
        let b = cpg_of(src, "python");
        let cmp = compare_cpg(&a, &b);
        assert_eq!(cmp.node_jaccard, 1.0);
        for (family, j) in &cmp.family_jaccards {
            assert_eq!(*j, 1.0, "family {family} jaccard < 1.0");
        }
        assert_eq!(cmp.dangerous_delta, 0.0);
        assert_eq!(cmp.taint_delta, 0.0);
        assert!(!cmp.changed());
    }

    #[test]
    fn cpg_compare_detects_added_dangerous_api() {
        let safe = "def f(x):\n    return x + 1\n";
        let unsafe_src = "def f(x):\n    eval(x)\n    return x + 1\n";
        let a = cpg_of(safe, "python");
        let b = cpg_of(unsafe_src, "python");
        assert!(dangerous_delta(&a, &b) >= 1.0);
    }

    #[test]
    fn cpg_compare_is_symmetric_for_identical() {
        for (language, src) in [
            ("python", "def f(): pass\n"),
            ("javascript", "function f() { return 1; }\n"),
        ] {
            let a = cpg_of(src, language);
            let b = cpg_of(src, language);
            let forward = compare_cpg(&a, &b);
            let backward = compare_cpg(&b, &a);
            assert_eq!(forward.node_jaccard, backward.node_jaccard);
            assert_eq!(forward.family_jaccards, backward.family_jaccards);
        }
    }
}
