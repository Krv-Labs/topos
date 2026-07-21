//! Graphify orphan-detection probe.
//!
//! Flags two kinds of structurally weak signal in a Graphify knowledge
//! graph: nodes with too few connections to be well-integrated into the
//! codebase's structure ("orphans"), and edges Graphify only *guesses* are
//! related rather than directly observing in the AST ("fragile" —
//! `INFERRED`/`AMBIGUOUS` confidence).
//!
//! Purely advisory — never folded into any [`crate::graphs::base::Representation::metrics`]
//! output or the SIMPLE/COMPOSABLE/SECURE score; only feeds
//! `topos refactor graphify` (issue #150).

use crate::graphs::graphify::{GraphifyConfidence, GraphifyGraph};

/// A node with total degree at or below the configured threshold.
#[derive(Debug, Clone, PartialEq)]
pub struct OrphanNode {
    pub node_id: String,
    pub label: String,
    pub degree: usize,
    pub source_file: Option<String>,
    pub source_location: Option<String>,
}

/// An edge Graphify only inferred or flagged as ambiguous, rather than
/// directly extracting from the AST.
#[derive(Debug, Clone, PartialEq)]
pub struct FragileEdge {
    pub source: String,
    pub target: String,
    pub relation: String,
    pub confidence: GraphifyConfidence,
}

/// Result of [`calculate_graphify_orphans`].
#[derive(Debug, Clone, Default)]
pub struct GraphifyOrphanResult {
    /// Ascending by degree — `0` (fully isolated) first, the most
    /// actionable row.
    pub orphan_nodes: Vec<OrphanNode>,
    /// `INFERRED` and `AMBIGUOUS` confidence only; `EXTRACTED` edges are
    /// never fragile by definition.
    pub fragile_edges: Vec<FragileEdge>,
}

/// Find low-degree nodes and low-confidence edges in `graph`.
///
/// `degree_threshold`: nodes with total (in + out) degree `<= degree_threshold`
/// count as orphans. `0` catches only fully isolated nodes; `1` (the
/// recommended default) also catches single-edge leaves, which are
/// structurally almost as weak.
pub fn calculate_graphify_orphans(
    graph: &GraphifyGraph,
    degree_threshold: usize,
) -> GraphifyOrphanResult {
    let mut orphan_nodes: Vec<OrphanNode> = graph
        .nodes
        .values()
        .filter_map(|node| {
            let degree = graph.degree(&node.id);
            (degree <= degree_threshold).then(|| OrphanNode {
                node_id: node.id.clone(),
                label: node.label.clone().unwrap_or_else(|| node.id.clone()),
                degree,
                source_file: node.source_file.clone(),
                source_location: node.source_location.clone(),
            })
        })
        .collect();
    orphan_nodes.sort_by(|a, b| {
        a.degree
            .cmp(&b.degree)
            .then_with(|| a.node_id.cmp(&b.node_id))
    });

    let fragile_edges: Vec<FragileEdge> = graph
        .edges
        .iter()
        .filter(|edge| {
            matches!(
                edge.confidence,
                GraphifyConfidence::Inferred | GraphifyConfidence::Ambiguous
            )
        })
        .map(|edge| FragileEdge {
            source: edge.source.clone(),
            target: edge.target.clone(),
            relation: edge
                .relation
                .clone()
                .unwrap_or_else(|| "unknown".to_string()),
            confidence: edge.confidence,
        })
        .collect();

    GraphifyOrphanResult {
        orphan_nodes,
        fragile_edges,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn graph_from(raw: &str) -> GraphifyGraph {
        GraphifyGraph::from_json_str(raw).unwrap()
    }

    #[test]
    fn isolated_node_is_an_orphan_at_threshold_zero() {
        let graph = graph_from(
            r#"{"nodes": [{"id": "a"}, {"id": "b"}, {"id": "c"}],
                "links": [{"source": "a", "target": "b"}]}"#,
        );
        let result = calculate_graphify_orphans(&graph, 0);
        let ids: Vec<&str> = result
            .orphan_nodes
            .iter()
            .map(|o| o.node_id.as_str())
            .collect();
        assert_eq!(ids, vec!["c"]);
    }

    #[test]
    fn single_edge_leaf_is_an_orphan_at_threshold_one_not_zero() {
        let graph = graph_from(
            r#"{"nodes": [{"id": "hub"}, {"id": "leaf"}, {"id": "isolated"}],
                "links": [{"source": "hub", "target": "leaf"}]}"#,
        );
        let at_zero = calculate_graphify_orphans(&graph, 0);
        let at_one = calculate_graphify_orphans(&graph, 1);
        let ids_zero: Vec<&str> = at_zero
            .orphan_nodes
            .iter()
            .map(|o| o.node_id.as_str())
            .collect();
        assert_eq!(ids_zero, vec!["isolated"]); // only the truly isolated node
        assert_eq!(at_one.orphan_nodes.len(), 3); // hub(1), leaf(1), isolated(0) all <= 1
    }

    #[test]
    fn orphans_sorted_ascending_by_degree() {
        let graph = graph_from(
            r#"{"nodes": [{"id": "a"}, {"id": "b"}, {"id": "c"}],
                "links": [{"source": "a", "target": "b"}, {"source": "b", "target": "c"}]}"#,
        );
        // degrees: a=1, b=2, c=1 — threshold 2 catches all three, ascending.
        let result = calculate_graphify_orphans(&graph, 2);
        let degrees: Vec<usize> = result.orphan_nodes.iter().map(|o| o.degree).collect();
        assert_eq!(degrees, vec![1, 1, 2]);
    }

    #[test]
    fn extracted_edges_are_never_fragile() {
        let graph = graph_from(
            r#"{"nodes": [{"id": "a"}, {"id": "b"}],
                "links": [{"source": "a", "target": "b", "confidence": "EXTRACTED"}]}"#,
        );
        let result = calculate_graphify_orphans(&graph, 0);
        assert!(result.fragile_edges.is_empty());
    }

    #[test]
    fn inferred_and_ambiguous_edges_are_both_fragile() {
        let graph = graph_from(
            r#"{"nodes": [{"id": "a"}, {"id": "b"}, {"id": "c"}],
                "links": [{"source": "a", "target": "b", "confidence": "INFERRED"},
                          {"source": "b", "target": "c", "confidence": "AMBIGUOUS"}]}"#,
        );
        let result = calculate_graphify_orphans(&graph, 0);
        assert_eq!(result.fragile_edges.len(), 2);
    }

    #[test]
    fn empty_graph_yields_empty_result() {
        let graph = graph_from(r#"{"nodes": [], "links": []}"#);
        let result = calculate_graphify_orphans(&graph, 1);
        assert!(result.orphan_nodes.is_empty());
        assert!(result.fragile_edges.is_empty());
    }
}
