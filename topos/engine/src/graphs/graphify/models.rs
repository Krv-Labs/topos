//! Graphify models — nodes and edges of a `graph.json` knowledge graph.
//!
//! Mirrors [`crate::graphs::mdg::models`]'s convention: manual
//! `serde_json::Value` field extraction rather than `#[derive(Deserialize)]`
//! (this crate depends on `serde_json` but not `serde`'s derive machinery),
//! and `relation` is documented, not enforced — Graphify's own vocabulary is
//! open and has grown across its 190+ pre-1.0 releases (`imports_from`,
//! `calls`, `inherits`, `uses`, `contains`, `method`, `dynamic_import`, ...),
//! so a closed Rust enum would need updating every time Graphify adds one.

use serde_json::Value;

/// Confidence tag Graphify attaches to each edge: `EXTRACTED` for a directly
/// AST-observed relationship, `INFERRED` for one the tool guesses at
/// (fragile — see [`crate::functors::probes::graphify::orphans`]),
/// `AMBIGUOUS` for the LLM semantic pass's uncertain matches. An edge with no
/// `confidence` field at all (schema drift, or a future Graphify version) is
/// [`GraphifyConfidence::Unknown`] rather than a parse error — we don't want
/// a new confidence value or a missing field to make the whole edge
/// unparseable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphifyConfidence {
    Extracted,
    Inferred,
    Ambiguous,
    Unknown,
}

impl GraphifyConfidence {
    fn from_str(raw: &str) -> Self {
        match raw {
            "EXTRACTED" => GraphifyConfidence::Extracted,
            "INFERRED" => GraphifyConfidence::Inferred,
            "AMBIGUOUS" => GraphifyConfidence::Ambiguous,
            _ => GraphifyConfidence::Unknown,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            GraphifyConfidence::Extracted => "EXTRACTED",
            GraphifyConfidence::Inferred => "INFERRED",
            GraphifyConfidence::Ambiguous => "AMBIGUOUS",
            GraphifyConfidence::Unknown => "UNKNOWN",
        }
    }
}

/// A node in a Graphify knowledge graph.
///
/// No `degree`/centrality field: Graphify's own `graph.json` doesn't carry
/// one (it's computed on the fly wherever Graphify itself displays it) — see
/// [`super::object::GraphifyGraph::degree`].
#[derive(Debug, Clone)]
pub struct GraphifyNode {
    pub id: String,
    pub label: Option<String>,
    pub file_type: Option<String>,
    pub source_file: Option<String>,
    pub source_location: Option<String>,
    /// Louvain (default) or Leiden (optional extra) community id. `None`
    /// when clustering hasn't run for this graph.
    pub community: Option<i64>,
    pub community_name: Option<String>,
}

/// An edge in a Graphify knowledge graph.
#[derive(Debug, Clone)]
pub struct GraphifyEdge {
    pub source: String,
    pub target: String,
    /// Open vocabulary (`"calls"`, `"imports_from"`, `"inherits"`, `"uses"`,
    /// `"contains"`, `"method"`, ...) — see the module doc.
    pub relation: Option<String>,
    pub confidence: GraphifyConfidence,
    pub weight: Option<f64>,
    pub source_file: Option<String>,
    pub source_location: Option<String>,
}

/// Parse one node record. Returns `None` only if `id` is missing/non-string
/// — every other field is optional, matching how sparse a real `graph.json`
/// node can be (a bare AST-extracted node may carry only `id`/`label`).
pub fn parse_node(item: &Value) -> Option<GraphifyNode> {
    Some(GraphifyNode {
        id: item.get("id")?.as_str()?.to_string(),
        label: item
            .get("label")
            .and_then(Value::as_str)
            .map(str::to_string),
        file_type: item
            .get("file_type")
            .and_then(Value::as_str)
            .map(str::to_string),
        source_file: item
            .get("source_file")
            .and_then(Value::as_str)
            .map(str::to_string),
        source_location: item
            .get("source_location")
            .and_then(Value::as_str)
            .map(str::to_string),
        community: item.get("community").and_then(Value::as_i64),
        community_name: item
            .get("community_name")
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}

/// Parse one edge record. Returns `None` only if `source`/`target` are
/// missing/non-string — the two fields every edge shape (past and present
/// Graphify schema versions) has always carried.
pub fn parse_edge(item: &Value) -> Option<GraphifyEdge> {
    Some(GraphifyEdge {
        source: item.get("source")?.as_str()?.to_string(),
        target: item.get("target")?.as_str()?.to_string(),
        relation: item
            .get("relation")
            .and_then(Value::as_str)
            .map(str::to_string),
        confidence: item
            .get("confidence")
            .and_then(Value::as_str)
            .map(GraphifyConfidence::from_str)
            .unwrap_or(GraphifyConfidence::Unknown),
        weight: item.get("weight").and_then(Value::as_f64),
        source_file: item
            .get("source_file")
            .and_then(Value::as_str)
            .map(str::to_string),
        source_location: item
            .get("source_location")
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}
