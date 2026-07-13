//! MDG Comparison — profunctor `D : E × E^op → ℝ` restricted to the
//! inter-module dependency graph.
//!
//! Pairwise comparison of two [`ModuleDependencyGraph`] instances. MDG
//! comparison answers questions of the form "did this refactor move
//! the file's architectural position in the codebase?" — coupling,
//! Martin instability, fan-in/out, and reachable-import depth.
//!
//! Signals (all `target − source` so a positive delta means *more*):
//!
//! - `coupling_delta` — signed change in `Ca + Ce`
//! - `instability_delta` — signed change in Martin instability `Ce/(Ca+Ce)`
//! - `fan_in_delta` — signed change in incoming CALLS edges
//! - `fan_out_delta` — signed change in outgoing CALLS edges
//! - `dep_depth_delta` — signed change in longest IMPORTS chain

use std::collections::HashMap;

use crate::graphs::base::Representation;
use crate::graphs::mdg::object::ModuleDependencyGraph;

/// Pairwise comparison summary for two module-dependency graphs.
#[derive(Debug, Clone, PartialEq)]
pub struct MDGComparison {
    pub coupling_delta: f64,
    pub instability_delta: f64,
    pub fan_in_delta: f64,
    pub fan_out_delta: f64,
    pub dep_depth_delta: f64,
    pub source_metrics: HashMap<String, f64>,
    pub target_metrics: HashMap<String, f64>,
}

impl MDGComparison {
    pub fn changed(&self) -> bool {
        [
            self.coupling_delta,
            self.instability_delta,
            self.fan_in_delta,
            self.fan_out_delta,
            self.dep_depth_delta,
        ]
        .iter()
        .any(|&delta| delta != 0.0)
    }
}

fn delta(source: &HashMap<String, f64>, target: &HashMap<String, f64>, key: &str) -> f64 {
    target.get(key).copied().unwrap_or(0.0) - source.get(key).copied().unwrap_or(0.0)
}

/// Signed change in total coupling `Ca + Ce` (target − source).
pub fn coupling_delta(source: &ModuleDependencyGraph, target: &ModuleDependencyGraph) -> f64 {
    delta(&source.metrics(), &target.metrics(), "mdg.coupling")
}

/// Signed change in Martin instability `Ce / (Ca + Ce)`.
pub fn instability_delta(source: &ModuleDependencyGraph, target: &ModuleDependencyGraph) -> f64 {
    delta(&source.metrics(), &target.metrics(), "mdg.instability")
}

/// Signed change in incoming CALLS edges.
pub fn fan_in_delta(source: &ModuleDependencyGraph, target: &ModuleDependencyGraph) -> f64 {
    delta(&source.metrics(), &target.metrics(), "mdg.fan_in")
}

/// Signed change in outgoing CALLS edges.
pub fn fan_out_delta(source: &ModuleDependencyGraph, target: &ModuleDependencyGraph) -> f64 {
    delta(&source.metrics(), &target.metrics(), "mdg.fan_out")
}

/// Signed change in longest IMPORTS chain length.
pub fn dep_depth_delta(source: &ModuleDependencyGraph, target: &ModuleDependencyGraph) -> f64 {
    delta(&source.metrics(), &target.metrics(), "mdg.dep_depth")
}

/// Run the full MDG comparison suite for a single pair of graphs.
pub fn compare_mdg(
    source: &ModuleDependencyGraph,
    target: &ModuleDependencyGraph,
) -> MDGComparison {
    let src_metrics = source.metrics();
    let tgt_metrics = target.metrics();
    MDGComparison {
        coupling_delta: delta(&src_metrics, &tgt_metrics, "mdg.coupling"),
        instability_delta: delta(&src_metrics, &tgt_metrics, "mdg.instability"),
        fan_in_delta: delta(&src_metrics, &tgt_metrics, "mdg.fan_in"),
        fan_out_delta: delta(&src_metrics, &tgt_metrics, "mdg.fan_out"),
        dep_depth_delta: delta(&src_metrics, &tgt_metrics, "mdg.dep_depth"),
        source_metrics: src_metrics,
        target_metrics: tgt_metrics,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphs::mdg::models::{GraphNode, GraphRelationship};
    use std::collections::HashMap as StdHashMap;

    fn empty_mdg(target_file: &str) -> ModuleDependencyGraph {
        let mut g = ModuleDependencyGraph::new(target_file);
        g.add_node(GraphNode {
            id: format!("File:{target_file}"),
            label: "File".to_string(),
            properties: StdHashMap::from([(
                "filePath".to_string(),
                serde_json::Value::String(target_file.to_string()),
            )]),
        });
        g
    }

    fn mdg_with_outgoing_import(target_file: &str) -> ModuleDependencyGraph {
        let mut g = empty_mdg(target_file);
        g.add_node(GraphNode {
            id: "File:other.py".to_string(),
            label: "File".to_string(),
            properties: StdHashMap::from([(
                "filePath".to_string(),
                serde_json::Value::String("other.py".to_string()),
            )]),
        });
        g.add_relationship(GraphRelationship {
            id: "i1".to_string(),
            source_id: format!("File:{target_file}"),
            target_id: "File:other.py".to_string(),
            rel_type: "IMPORTS".to_string(),
            confidence: 1.0,
            reason: String::new(),
        });
        g
    }

    #[test]
    fn mdg_compare_identical_isolated() {
        let a = empty_mdg("a.py");
        let b = empty_mdg("a.py");
        let cmp = compare_mdg(&a, &b);
        assert_eq!(cmp.coupling_delta, 0.0);
        assert_eq!(cmp.fan_in_delta, 0.0);
        assert_eq!(cmp.fan_out_delta, 0.0);
        assert!(!cmp.changed());
    }

    #[test]
    fn mdg_compare_detects_added_import_chain() {
        let a = empty_mdg("a.py");
        let b = mdg_with_outgoing_import("a.py");
        let cmp = compare_mdg(&a, &b);
        // Either the dep_depth or instability moved.
        assert!(cmp.dep_depth_delta > 0.0 || cmp.instability_delta != 0.0);
    }
}
