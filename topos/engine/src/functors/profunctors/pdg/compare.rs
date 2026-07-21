//! PDG Comparison — profunctor `D : E × E^op → ℝ` restricted to the
//! academic intra-procedural Program Dependence Graph.
//!
//! Pairwise comparison of two [`ProgramDependenceGraph`] instances.
//! PDG-level comparison surfaces whether a refactor preserved the
//! def-use and predicate-executor wiring even when the AST has been
//! rewritten.
//!
//! Signals:
//!
//! - `data_dep_jaccard` — Jaccard similarity over data-dependence edges
//! - `control_dep_jaccard` — Jaccard similarity over control-dependence
//!   edges
//! - `statement_delta` — signed change in statement-node count
//! - `density_delta` — signed change in normalized dependence density
//!
//! Edge equality is by the triple `(source_id, target_id, var)` for
//! data edges and `(source_id, target_id)` for control edges — i.e.
//! structural identity using the stable UAST node ids assigned during
//! parsing.

use std::collections::{HashMap, HashSet};

use crate::graphs::base::Representation;
use crate::graphs::pdg::object::{DependenceEdge, DependenceKind, ProgramDependenceGraph};

/// Pairwise comparison summary for two program-dependence graphs.
#[derive(Debug, Clone, PartialEq)]
pub struct PDGComparison {
    pub data_dep_jaccard: f64,
    pub control_dep_jaccard: f64,
    pub statement_delta: i64,
    pub density_delta: f64,
    pub source_metrics: HashMap<String, f64>,
    pub target_metrics: HashMap<String, f64>,
}

impl PDGComparison {
    pub fn changed(&self) -> bool {
        self.data_dep_jaccard < 1.0
            || self.control_dep_jaccard < 1.0
            || self.statement_delta != 0
            || self.density_delta != 0.0
    }
}

fn data_edge_key(edge: &DependenceEdge) -> (&str, &str, &str) {
    (
        edge.source.as_str(),
        edge.target.as_str(),
        edge.var.as_str(),
    )
}

fn control_edge_key(edge: &DependenceEdge) -> (&str, &str) {
    (edge.source.as_str(), edge.target.as_str())
}

fn jaccard<T: Eq + std::hash::Hash>(a: &HashSet<T>, b: &HashSet<T>) -> f64 {
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    let intersection = a.intersection(b).count();
    let union = a.union(b).count();
    intersection as f64 / union as f64
}

/// Jaccard similarity over `(def → use, var)` data-dependence triples.
pub fn data_dep_jaccard(source: &ProgramDependenceGraph, target: &ProgramDependenceGraph) -> f64 {
    let a: HashSet<_> = source
        .edges
        .iter()
        .filter(|e| e.kind == DependenceKind::Data)
        .map(data_edge_key)
        .collect();
    let b: HashSet<_> = target
        .edges
        .iter()
        .filter(|e| e.kind == DependenceKind::Data)
        .map(data_edge_key)
        .collect();
    jaccard(&a, &b)
}

/// Jaccard similarity over `(predicate → executor)` control-dependence
/// pairs.
pub fn control_dep_jaccard(
    source: &ProgramDependenceGraph,
    target: &ProgramDependenceGraph,
) -> f64 {
    let a: HashSet<_> = source
        .edges
        .iter()
        .filter(|e| e.kind == DependenceKind::Control)
        .map(control_edge_key)
        .collect();
    let b: HashSet<_> = target
        .edges
        .iter()
        .filter(|e| e.kind == DependenceKind::Control)
        .map(control_edge_key)
        .collect();
    jaccard(&a, &b)
}

/// Signed change in statement-node count (target − source).
pub fn statement_delta(source: &ProgramDependenceGraph, target: &ProgramDependenceGraph) -> i64 {
    target.statements.len() as i64 - source.statements.len() as i64
}

/// Signed change in normalized dependence density (target − source).
pub fn density_delta(source: &ProgramDependenceGraph, target: &ProgramDependenceGraph) -> f64 {
    target.metrics()["pdg.density"] - source.metrics()["pdg.density"]
}

/// Run the full PDG comparison suite for a single pair of graphs.
pub fn compare_pdg(
    source: &ProgramDependenceGraph,
    target: &ProgramDependenceGraph,
) -> PDGComparison {
    PDGComparison {
        data_dep_jaccard: data_dep_jaccard(source, target),
        control_dep_jaccard: control_dep_jaccard(source, target),
        statement_delta: statement_delta(source, target),
        density_delta: density_delta(source, target),
        source_metrics: source.metrics(),
        target_metrics: target.metrics(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::morphism::ProgramMorphism;

    fn pdg_of(source: &str) -> ProgramDependenceGraph {
        let mut morphism = ProgramMorphism::new(source, "python");
        morphism.build_pdg().unwrap().clone()
    }

    #[test]
    fn pdg_compare_identical_is_full_jaccard() {
        let src = "def f(x):\n    y = x + 1\n    if x > 0:\n        y = y * 2\n    return y\n";
        let a = pdg_of(src);
        let b = pdg_of(src);
        let cmp = compare_pdg(&a, &b);
        assert_eq!(cmp.data_dep_jaccard, 1.0);
        assert_eq!(cmp.control_dep_jaccard, 1.0);
        assert_eq!(cmp.statement_delta, 0);
    }

    #[test]
    fn pdg_data_dep_jaccard_is_well_defined() {
        let a = pdg_of("def f(x):\n    return x + 1\n");
        let b = pdg_of("def g(y):\n    return y + 2\n");
        let j = data_dep_jaccard(&a, &b);
        assert!((0.0..=1.0).contains(&j));
    }

    #[test]
    fn pdg_control_dep_jaccard_drops_when_branch_added() {
        let flat = pdg_of("def f(x):\n    return x\n");
        let branchy = pdg_of("def f(x):\n    if x > 0:\n        return 1\n    return 0\n");
        let cmp = compare_pdg(&flat, &branchy);
        assert!(cmp.control_dep_jaccard < 1.0);
    }
}
