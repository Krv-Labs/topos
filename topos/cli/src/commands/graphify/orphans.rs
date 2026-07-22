//! `topos graphify orphans` — list orphan nodes / fragile edges for a file.
//!
//! Split out of `graphify.rs` alongside [`super::generate`] -- see that
//! module's doc comment for why.

use std::path::PathBuf;

use clap::Args;
use topos_engine::adapters::graphify::GRAPHIFY_GRAPH_FILE;
use topos_engine::functors::probes::graphify::orphans::{
    calculate_graphify_orphans, FragileEdge, GraphifyOrphanResult, OrphanNode,
};
use topos_engine::graphs::graphify::GraphifyGraph;

use super::print_json;

const DEFAULT_ORPHAN_DEGREE_THRESHOLD: usize = 1;

#[derive(Args)]
pub struct OrphansArgs {
    /// The file to scope orphan nodes / fragile edges to (matched against
    /// each node/edge's `source_file`).
    pub filepath: PathBuf,
    /// Directory containing `graph.json` (default: `./graphify-out`).
    #[arg(long)]
    pub graphify_dir: Option<PathBuf>,
    #[arg(long, default_value_t = 5)]
    pub limit: usize,
    /// Output the result as a single JSON object.
    #[arg(long)]
    pub json: bool,
}

/// Scope orphan nodes / fragile edges to `filepath`, truncated to `limit`.
///
/// `FragileEdge.source`/`target` are Graphify node ids, not file paths — an
/// edge is scoped in by looking up each endpoint's own `node.source_file`
/// in `graph`, not by comparing the edge's fields directly against
/// `filepath`.
fn scope_to_file<'a>(
    graph: &GraphifyGraph,
    result: &'a GraphifyOrphanResult,
    filepath: &str,
    limit: usize,
) -> (Vec<&'a OrphanNode>, Vec<&'a FragileEdge>) {
    let orphan_nodes: Vec<&OrphanNode> = result
        .orphan_nodes
        .iter()
        .filter(|node| node.source_file.as_deref() == Some(filepath))
        .take(limit)
        .collect();
    let fragile_edges: Vec<&FragileEdge> = result
        .fragile_edges
        .iter()
        .filter(|edge| {
            [&edge.source, &edge.target].into_iter().any(|id| {
                graph
                    .node(id)
                    .and_then(|n| n.source_file.as_deref())
                    .is_some_and(|f| f == filepath)
            })
        })
        .take(limit)
        .collect();
    (orphan_nodes, fragile_edges)
}

pub fn run_orphans(args: OrphansArgs) -> Result<(), String> {
    let graphify_dir = args
        .graphify_dir
        .unwrap_or_else(|| PathBuf::from("graphify-out"));
    let graph = GraphifyGraph::from_json_file(graphify_dir.join(GRAPHIFY_GRAPH_FILE))
        .map_err(|e| e.to_string())?;

    let result = calculate_graphify_orphans(&graph, DEFAULT_ORPHAN_DEGREE_THRESHOLD);
    let filepath = args.filepath.to_string_lossy().to_string();
    let (orphan_nodes, fragile_edges) = scope_to_file(&graph, &result, &filepath, args.limit);

    if args.json {
        let nodes_json: Vec<_> = orphan_nodes
            .iter()
            .map(|n| {
                serde_json::json!({
                    "node_id": n.node_id, "label": n.label, "degree": n.degree,
                    "source_file": n.source_file, "source_location": n.source_location,
                })
            })
            .collect();
        let edges_json: Vec<_> = fragile_edges
            .iter()
            .map(|e| {
                serde_json::json!({
                    "source": e.source, "target": e.target,
                    "relation": e.relation, "confidence": e.confidence.as_str(),
                })
            })
            .collect();
        print_json(&serde_json::json!({
            "orphan_nodes": nodes_json,
            "fragile_edges": edges_json,
        }))?;
        return Ok(());
    }

    println!("Orphan nodes ({})", filepath);
    println!("{}", "-".repeat(40));
    if orphan_nodes.is_empty() {
        println!("  (none)");
    }
    for node in &orphan_nodes {
        println!(
            "  [{}] {} (degree {})",
            node.node_id, node.label, node.degree
        );
    }

    println!();
    println!("Fragile edges (INFERRED | AMBIGUOUS)");
    println!("{}", "-".repeat(40));
    if fragile_edges.is_empty() {
        println!("  (none)");
    }
    for edge in &fragile_edges {
        println!(
            "  {} -> {} ({}, {})",
            edge.source,
            edge.target,
            edge.relation,
            edge.confidence.as_str()
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = r#"{
        "nodes": [
            {"id": "a", "label": "a()", "source_file": "src/a.rs"},
            {"id": "b", "label": "b()", "source_file": "src/b.rs"},
            {"id": "c", "label": "c()", "source_file": "src/a.rs"}
        ],
        "links": [
            {"source": "a", "target": "b", "confidence": "INFERRED", "relation": "calls"},
            {"source": "b", "target": "c", "confidence": "EXTRACTED", "relation": "calls"}
        ]
    }"#;

    #[test]
    fn scope_to_file_smoke_test() {
        let graph = GraphifyGraph::from_json_str(FIXTURE).unwrap();
        let result = calculate_graphify_orphans(&graph, DEFAULT_ORPHAN_DEGREE_THRESHOLD);

        let (nodes, edges) = scope_to_file(&graph, &result, "src/a.rs", 5);
        // Both "a" and "c" live in src/a.rs and are degree-1 orphans;
        // "b" (src/b.rs) must not leak in.
        let node_ids: Vec<&str> = nodes.iter().map(|n| n.node_id.as_str()).collect();
        assert_eq!(node_ids, vec!["a", "c"]);

        // Only the INFERRED edge (a -> b) is fragile at all; it touches
        // src/a.rs via its source endpoint "a", even though "a" itself
        // isn't a path in the edge's own source/target fields.
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].source, "a");
        assert_eq!(edges[0].target, "b");

        // "b" has degree 2 (touches both edges) so it's not an orphan
        // itself, but src/b.rs still surfaces the fragile edge via "b"'s
        // target endpoint.
        let (b_nodes, b_edges) = scope_to_file(&graph, &result, "src/b.rs", 5);
        assert!(b_nodes.is_empty());
        assert_eq!(b_edges.len(), 1);

        // limit truncates.
        let (limited, _) = scope_to_file(&graph, &result, "src/a.rs", 1);
        assert_eq!(limited.len(), 1);
    }

    #[test]
    fn scope_to_file_returns_nothing_for_an_unknown_file() {
        let graph = GraphifyGraph::from_json_str(FIXTURE).unwrap();
        let result = calculate_graphify_orphans(&graph, DEFAULT_ORPHAN_DEGREE_THRESHOLD);
        let (nodes, edges) = scope_to_file(&graph, &result, "src/nonexistent.rs", 5);
        assert!(nodes.is_empty());
        assert!(edges.is_empty());
    }
}
