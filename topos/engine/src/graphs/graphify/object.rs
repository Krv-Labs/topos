//! Graphify graph representation, parsed from a `graphify-out/graph.json`
//! file.
//!
//! This is a **refactoring-tool** input, not a scored
//! [`crate::graphs::base::Representation`]: Graphify-derived analysis (issue
//! #150) must never influence the SIMPLE, COMPOSABLE, or SECURE medal
//! computation. It exists purely to feed the orphan-detection probe
//! ([`crate::functors::probes::graphify::orphans`]) that powers
//! `topos refactor graphify`.
//!
//! Unlike [`crate::graphs::process::ProcessGraph`] (which filters an
//! already-loaded [`crate::graphs::mdg::ModuleDependencyGraph`]),
//! `graph.json` is a wholly independent data source — GitNexus's `.gitnexus`
//! store and Graphify's `graphify-out/` are two unrelated external tools'
//! output, not two views of the same graph. So this type parses `graph.json`
//! from scratch rather than filtering an existing representation.
//!
//! `graph.json` is literally networkx's `node_link_data()` JSON format.
//! Verified against a real install's output (see the adapter's module doc)
//! and against Graphify's own changelog history of breaking schema changes
//! across its 190+ pre-1.0 releases, two defensive choices follow directly:
//! the edge-array key has flip-flopped between `"links"` and `"edges"`
//! historically, so both are accepted (preferring `links`); and the
//! top-level `"directed"` flag is unreliable (observed `false` even for
//! logically-directed relations like `calls`/`imports_from`) and is never
//! consulted — every edge is treated as directed in our own model.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::models::{parse_edge, parse_node, GraphifyEdge, GraphifyNode};

/// Failure to load/parse a `graph.json` file.
#[derive(Debug)]
pub enum GraphifyError {
    NotFound(PathBuf),
    Io(std::io::Error),
    Parse(serde_json::Error),
    /// Neither a `"links"` nor an `"edges"` top-level array was present.
    MissingEdgeKey,
}

impl std::fmt::Display for GraphifyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GraphifyError::NotFound(path) => write!(
                f,
                "Graphify graph not found at {}. Run 'graphify update' (or \
                 'topos graphify generate') in the repository root first.",
                path.display()
            ),
            GraphifyError::Io(e) => write!(f, "{e}"),
            GraphifyError::Parse(e) => write!(f, "invalid graph.json: {e}"),
            GraphifyError::MissingEdgeKey => {
                write!(f, "graph.json has neither a \"links\" nor \"edges\" array")
            }
        }
    }
}

impl std::error::Error for GraphifyError {}

/// A Graphify knowledge graph: nodes + edges, with degree computed in Rust
/// since Graphify's own `graph.json` carries no degree/centrality field.
#[derive(Debug, Clone, Default)]
pub struct GraphifyGraph {
    pub nodes: HashMap<String, GraphifyNode>,
    pub edges: Vec<GraphifyEdge>,
    degree: HashMap<String, usize>,
}

impl GraphifyGraph {
    /// Load and parse a `graph.json` file from disk.
    pub fn from_json_file(path: impl AsRef<Path>) -> Result<Self, GraphifyError> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                GraphifyError::NotFound(path.to_path_buf())
            } else {
                GraphifyError::Io(e)
            }
        })?;
        Self::from_json_str(&text)
    }

    /// Parse a `graph.json` document already read into memory.
    pub fn from_json_str(raw: &str) -> Result<Self, GraphifyError> {
        let value: Value = serde_json::from_str(raw).map_err(GraphifyError::Parse)?;

        let nodes_json = value
            .get("nodes")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let mut nodes = HashMap::with_capacity(nodes_json.len());
        for item in nodes_json {
            if let Some(node) = parse_node(item) {
                nodes.insert(node.id.clone(), node);
            }
        }

        // The edge-array key has flip-flopped between "links" and "edges"
        // across Graphify's own history — accept either, preferring
        // "links" (the current default). An empty array under either key is
        // valid (a graph can legitimately have zero edges); only the
        // complete absence of both keys is an error.
        let edges_json = match value.get("links").or_else(|| value.get("edges")) {
            Some(arr) => arr.as_array().map(Vec::as_slice).unwrap_or(&[]),
            None => return Err(GraphifyError::MissingEdgeKey),
        };
        let mut edges = Vec::with_capacity(edges_json.len());
        let mut degree: HashMap<String, usize> = HashMap::new();
        for item in edges_json {
            let Some(edge) = parse_edge(item) else {
                continue;
            };
            *degree.entry(edge.source.clone()).or_insert(0) += 1;
            *degree.entry(edge.target.clone()).or_insert(0) += 1;
            edges.push(edge);
        }

        Ok(GraphifyGraph {
            nodes,
            edges,
            degree,
        })
    }

    /// Total (in + out) edge count touching `node_id`. `0` for a node with
    /// no edges, or an id not present in the graph at all.
    pub fn degree(&self, node_id: &str) -> usize {
        self.degree.get(node_id).copied().unwrap_or(0)
    }

    pub fn node(&self, id: &str) -> Option<&GraphifyNode> {
        self.nodes.get(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphs::graphify::models::GraphifyConfidence;

    /// A real (trimmed) sample from `graphify update` output — see the
    /// adapter module doc for how it was obtained. Exercises `links`, all
    /// three confidence values via an added synthetic edge, and extra
    /// fields (`_origin`, `norm_label`, `confidence_score`, top-level
    /// `hyperedges`) that a strict schema would choke on.
    const REAL_SAMPLE: &str = r#"{
        "directed": false,
        "multigraph": false,
        "graph": {},
        "nodes": [
            {"label": "sample.py", "file_type": "code", "source_file": "sample.py",
             "source_location": "L1", "_origin": "ast", "id": "sample",
             "community": 0, "community_name": "sample.py", "norm_label": "sample.py"},
            {"label": "add()", "file_type": "code", "source_file": "sample.py",
             "source_location": "L1", "_origin": "ast", "id": "sample_add",
             "community": 0, "community_name": "sample.py", "norm_label": "add()"},
            {"label": "Greeter", "file_type": "code", "source_file": "sample.py",
             "source_location": "L4", "_origin": "ast", "id": "sample_greeter",
             "community": 1, "community_name": "Greeter", "norm_label": "greeter"}
        ],
        "links": [
            {"relation": "contains", "confidence": "EXTRACTED", "source_file": "sample.py",
             "source_location": "L1", "weight": 1.0, "_origin": "ast",
             "source": "sample", "target": "sample_add", "confidence_score": 1.0},
            {"relation": "contains", "confidence": "EXTRACTED", "source_file": "sample.py",
             "source_location": "L4", "weight": 1.0, "_origin": "ast",
             "source": "sample", "target": "sample_greeter", "confidence_score": 1.0},
            {"relation": "uses", "confidence": "INFERRED", "source": "sample_add",
             "target": "sample_greeter", "weight": 0.5}
        ],
        "hyperedges": []
    }"#;

    #[test]
    fn parses_real_sample_with_links_key() {
        let graph = GraphifyGraph::from_json_str(REAL_SAMPLE).unwrap();
        assert_eq!(graph.nodes.len(), 3);
        assert_eq!(graph.edges.len(), 3);
        assert_eq!(
            graph.node("sample").unwrap().label.as_deref(),
            Some("sample.py")
        );
        assert_eq!(graph.node("sample").unwrap().community, Some(0));
    }

    #[test]
    fn accepts_edges_key_as_a_fallback() {
        let raw = r#"{"nodes": [{"id": "a"}, {"id": "b"}],
                       "edges": [{"source": "a", "target": "b"}]}"#;
        let graph = GraphifyGraph::from_json_str(raw).unwrap();
        assert_eq!(graph.edges.len(), 1);
        assert_eq!(graph.degree("a"), 1);
        assert_eq!(graph.degree("b"), 1);
    }

    #[test]
    fn missing_edge_key_is_an_error() {
        let raw = r#"{"nodes": [{"id": "a"}]}"#;
        let err = GraphifyGraph::from_json_str(raw).unwrap_err();
        assert!(matches!(err, GraphifyError::MissingEdgeKey));
    }

    #[test]
    fn empty_edge_array_is_not_an_error() {
        let raw = r#"{"nodes": [{"id": "a"}], "links": []}"#;
        let graph = GraphifyGraph::from_json_str(raw).unwrap();
        assert!(graph.edges.is_empty());
        assert_eq!(graph.degree("a"), 0);
    }

    #[test]
    fn directed_flag_in_source_is_ignored() {
        // The sample has "directed": false but relations (contains, uses)
        // are logically directed — we don't consult the flag at all, so
        // parsing succeeds identically regardless of its value.
        let graph = GraphifyGraph::from_json_str(REAL_SAMPLE).unwrap();
        assert_eq!(graph.edges.len(), 3);
    }

    #[test]
    fn node_missing_community_parses_with_none() {
        let raw = r#"{"nodes": [{"id": "a", "label": "a"}], "links": []}"#;
        let graph = GraphifyGraph::from_json_str(raw).unwrap();
        assert_eq!(graph.node("a").unwrap().community, None);
    }

    #[test]
    fn all_three_confidence_values_parse() {
        let graph = GraphifyGraph::from_json_str(REAL_SAMPLE).unwrap();
        let confidences: Vec<GraphifyConfidence> =
            graph.edges.iter().map(|e| e.confidence).collect();
        assert!(confidences.contains(&GraphifyConfidence::Extracted));
        assert!(confidences.contains(&GraphifyConfidence::Inferred));
    }

    #[test]
    fn unknown_confidence_value_does_not_fail_the_parse() {
        let raw = r#"{"nodes": [{"id": "a"}, {"id": "b"}],
                       "links": [{"source": "a", "target": "b", "confidence": "SOMETHING_NEW"}]}"#;
        let graph = GraphifyGraph::from_json_str(raw).unwrap();
        assert_eq!(graph.edges[0].confidence, GraphifyConfidence::Unknown);
    }

    #[test]
    fn degree_counts_both_endpoints_of_a_small_graph() {
        let raw = r#"{"nodes": [{"id": "a"}, {"id": "b"}, {"id": "c"}],
                       "links": [{"source": "a", "target": "b"},
                                 {"source": "b", "target": "c"}]}"#;
        let graph = GraphifyGraph::from_json_str(raw).unwrap();
        assert_eq!(graph.degree("a"), 1);
        assert_eq!(graph.degree("b"), 2);
        assert_eq!(graph.degree("c"), 1);
        assert_eq!(graph.degree("nonexistent"), 0);
    }

    #[test]
    fn missing_file_is_not_found() {
        let err = GraphifyGraph::from_json_file("/nonexistent/graph.json").unwrap_err();
        assert!(matches!(err, GraphifyError::NotFound(_)));
    }

    #[test]
    fn malformed_json_is_a_parse_error() {
        let err = GraphifyGraph::from_json_str("{not valid json").unwrap_err();
        assert!(matches!(err, GraphifyError::Parse(_)));
    }
}
