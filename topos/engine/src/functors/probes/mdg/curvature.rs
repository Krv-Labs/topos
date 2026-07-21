//! MDG curvature probe.
//!
//! Applies balanced Forman curvature (Topping, Di Giovanni, Chamberlain,
//! Dong & Bronstein, "Understanding over-squashing and bottlenecks on graphs
//! via curvature", ICLR 2022, arxiv:2111.14522) to the file-level dependency
//! graph, to name concrete edges worth strengthening.
//!
//! The paper uses this curvature to find GNN message-passing bottlenecks —
//! edges with very negative curvature "squash" information from
//! exponentially many distant neighborhoods through a single transition.
//! The same signal, applied to module dependencies instead of message
//! passing, flags load-bearing import edges: highly negative curvature means
//! many otherwise-unrelated modules route their coupling through this one
//! dependency.
//!
//! Purely advisory — never folded into `mdg.*` metrics or the COMPOSABLE
//! score; only feeds `topos refactor dependencies`.

use std::collections::{HashMap, HashSet};

use crate::functors::curvature::{balanced_forman_curvature, WeightedEdge};
use crate::functors::probes::mdg::coupling::owning_file;
use crate::graphs::mdg::object::ModuleDependencyGraph;

/// Curvature per file-level dependency edge touching the target file.
///
/// `edges` holds `(source_file_id, target_file_id, curvature)` tuples,
/// sorted ascending by curvature (most negative — the strongest "strengthen
/// this" signal — first).
#[derive(Debug, Clone, Default)]
pub struct MdgCurvatureResult {
    pub edges: Vec<(String, String, f64)>,
}

/// Compute balanced Forman curvature for every dependency edge touching
/// `file_node_id`.
///
/// Builds the whole project's file-level dependency graph (resolving
/// symbol-level `rel_type` edges, e.g. `IMPORTS`, up to their owning File
/// nodes, matching [`crate::functors::probes::mdg::coupling`]'s approach) so
/// that curvature at each edge reflects its true local neighborhood rather
/// than a truncated one-file ego network, then filters the result down to
/// edges incident to `file_node_id`.
pub fn calculate_mdg_curvature(
    graph: &ModuleDependencyGraph,
    file_node_id: &str,
    rel_type: &str,
) -> MdgCurvatureResult {
    let file_ids: Vec<&str> = graph
        .nodes
        .values()
        .filter(|n| n.label == "File")
        .map(|n| n.id.as_str())
        .collect();
    let id_to_idx: HashMap<&str, usize> = file_ids
        .iter()
        .enumerate()
        .map(|(i, &fid)| (fid, i))
        .collect();
    let Some(&target_idx) = id_to_idx.get(file_node_id) else {
        return MdgCurvatureResult::default();
    };

    let mut edge_pairs: HashSet<(usize, usize)> = HashSet::new();
    for &fid in &file_ids {
        let mut symbol_ids: HashSet<String> =
            graph.all_contained_symbols(fid).into_iter().collect();
        symbol_ids.insert(fid.to_string());
        for sid in &symbol_ids {
            for rel in graph.outgoing(sid, Some(rel_type)) {
                if let Some(target_file) = owning_file(graph, &rel.target_id) {
                    if target_file != fid {
                        if let Some(&t) = id_to_idx.get(target_file.as_str()) {
                            let a = id_to_idx[fid];
                            edge_pairs.insert((a.min(t), a.max(t)));
                        }
                    }
                }
            }
        }
    }

    let weighted_edges: Vec<WeightedEdge> = edge_pairs
        .iter()
        .map(|&(a, b)| WeightedEdge::new(a, b, 1.0))
        .collect();
    let curvatures = balanced_forman_curvature(&weighted_edges);

    let mut edges: Vec<(String, String, f64)> = curvatures
        .iter()
        .filter(|c| c.source == target_idx || c.target == target_idx)
        .map(|c| {
            (
                file_ids[c.source].to_string(),
                file_ids[c.target].to_string(),
                c.curvature,
            )
        })
        .collect();
    edges.sort_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal));
    MdgCurvatureResult { edges }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphs::mdg::models::{GraphNode, GraphRelationship};

    fn file_node(id: &str) -> GraphNode {
        GraphNode {
            id: id.to_string(),
            label: "File".to_string(),
            properties: HashMap::new(),
        }
    }

    fn imports(source: &str, target: &str) -> GraphRelationship {
        GraphRelationship {
            id: format!("{source}->{target}"),
            source_id: source.to_string(),
            target_id: target.to_string(),
            rel_type: "IMPORTS".to_string(),
            confidence: 1.0,
            reason: String::new(),
            properties: HashMap::new(),
        }
    }

    /// A bridge between two file triangles is the most negative edge
    /// incident to the bridging file.
    #[test]
    fn bridge_edge_ranks_most_negative() {
        let mut g = ModuleDependencyGraph::new("a.py");
        for id in ["f:a", "f:b", "f:c", "f:d", "f:e", "f:f"] {
            g.add_node(file_node(id));
        }
        // triangle 1
        g.add_relationship(imports("f:a", "f:b"));
        g.add_relationship(imports("f:b", "f:c"));
        g.add_relationship(imports("f:c", "f:a"));
        // triangle 2
        g.add_relationship(imports("f:d", "f:e"));
        g.add_relationship(imports("f:e", "f:f"));
        g.add_relationship(imports("f:f", "f:d"));
        // bridge
        g.add_relationship(imports("f:c", "f:d"));

        let result = calculate_mdg_curvature(&g, "f:c", "IMPORTS");
        assert!(!result.edges.is_empty());
        let (s, t, _) = &result.edges[0];
        let bridge = (s.as_str(), t.as_str());
        assert!(
            bridge == ("f:c", "f:d") || bridge == ("f:d", "f:c"),
            "expected the bridge first, got {bridge:?}"
        );
    }

    #[test]
    fn unknown_file_yields_empty() {
        let g = ModuleDependencyGraph::new("a.py");
        let result = calculate_mdg_curvature(&g, "f:missing", "IMPORTS");
        assert!(result.edges.is_empty());
    }
}
