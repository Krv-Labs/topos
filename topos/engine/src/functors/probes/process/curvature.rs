//! Process-graph curvature probe.
//!
//! Applies directed Forman-Ricci curvature (Samal et al.) to GitNexus
//! process graphs to find execution "choke points": single transitions where
//! many independent call paths squeeze through. Node/edge weights default to
//! uniform 1.0 (no call-frequency or timing data exists in GitNexus's schema
//! today); this degenerates the formula to unweighted directed Forman
//! curvature while keeping the weighted shape for future data.
//!
//! Purely advisory — process-graph curvature never influences
//! SIMPLE/COMPOSABLE/SECURE scoring; it only feeds `topos refactor process`.

use std::collections::HashMap;

use crate::functors::curvature::{directed_forman_curvature, WeightedEdge};
use crate::graphs::process::object::ProcessGraph;

/// Curvature per process-graph transition.
///
/// `edges` holds `(source_node_id, target_node_id, curvature)` tuples,
/// sorted ascending by curvature (most negative — the strongest "choke
/// point" signal — first).
#[derive(Debug, Clone, Default)]
pub struct ProcessCurvatureResult {
    pub edges: Vec<(String, String, f64)>,
}

/// Compute directed Forman-Ricci curvature for every transition in `graph`.
///
/// Interns string node ids to the dense integers the engine expects, calls
/// [`directed_forman_curvature`], then de-interns the results.
/// `node_weights` optionally overrides per-node weights (keyed by node id;
/// defaults to uniform 1.0).
pub fn calculate_process_curvature(
    graph: &ProcessGraph,
    node_weights: Option<&HashMap<String, f64>>,
) -> ProcessCurvatureResult {
    let edges = graph.edges();
    if edges.is_empty() {
        return ProcessCurvatureResult::default();
    }

    let mut node_ids: Vec<&str> = Vec::new();
    let mut id_to_idx: HashMap<&str, usize> = HashMap::new();
    for &(source, target) in &edges {
        for node_id in [source, target] {
            if !id_to_idx.contains_key(node_id) {
                id_to_idx.insert(node_id, node_ids.len());
                node_ids.push(node_id);
            }
        }
    }

    let idx_weights: HashMap<usize, f64> = node_weights
        .map(|weights| {
            id_to_idx
                .iter()
                .filter_map(|(node_id, &idx)| weights.get(*node_id).map(|&w| (idx, w)))
                .collect()
        })
        .unwrap_or_default();

    let weighted_edges: Vec<WeightedEdge> = edges
        .iter()
        .map(|&(source, target)| WeightedEdge::new(id_to_idx[source], id_to_idx[target], 1.0))
        .collect();
    let curvatures = directed_forman_curvature(
        &weighted_edges,
        (!idx_weights.is_empty()).then_some(&idx_weights),
    );

    let mut result_edges: Vec<(String, String, f64)> = curvatures
        .iter()
        .map(|c| {
            (
                node_ids[c.source].to_string(),
                node_ids[c.target].to_string(),
                c.curvature,
            )
        })
        .collect();
    result_edges.sort_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal));
    ProcessCurvatureResult {
        edges: result_edges,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphs::process::object::{ProcessPath, ProcessStep};

    fn step(node_id: &str, ord: i64) -> ProcessStep {
        ProcessStep {
            node_id: node_id.to_string(),
            label: "Function".to_string(),
            step: ord,
            properties: HashMap::new(),
        }
    }

    fn path(process_id: &str, nodes: &[&str]) -> ProcessPath {
        ProcessPath {
            process_id: process_id.to_string(),
            steps: nodes
                .iter()
                .enumerate()
                .map(|(i, n)| step(n, i as i64))
                .collect(),
        }
    }

    /// Two paths funneling through the same middle transition make that
    /// transition the most negative edge.
    #[test]
    fn shared_transition_is_choke_point() {
        let graph = ProcessGraph::from_paths(
            "main.py",
            vec![
                path("p1", &["a", "hub", "spoke", "x"]),
                path("p2", &["b", "hub", "spoke", "y"]),
            ],
        );
        let result = calculate_process_curvature(&graph, None);
        assert!(!result.edges.is_empty());
        let (s, t, _) = &result.edges[0];
        assert_eq!((s.as_str(), t.as_str()), ("hub", "spoke"));
    }

    #[test]
    fn empty_graph_yields_empty_result() {
        let graph = ProcessGraph::from_paths("main.py", vec![]);
        assert!(calculate_process_curvature(&graph, None).edges.is_empty());
    }
}
