//! MDG models — nodes and relationships of the GitNexus knowledge graph.
//!
//! `NodeLabel`/`RelationshipType` are documentation constants, not a
//! closed Rust enum: Python's originals are `Literal[...]` type hints,
//! which aren't runtime-enforced either — GitNexus can emit labels
//! outside this list, and both languages pass them through as plain
//! strings rather than rejecting the unrecognized ones.

use serde_json::Value;
use std::collections::HashMap;

/// Common node labels GitNexus emits (not exhaustive — see the module doc).
pub const NODE_LABELS: &[&str] = &[
    "Project",
    "Package",
    "Module",
    "Folder",
    "File",
    "Class",
    "Function",
    "Method",
    "Variable",
    "Interface",
    "Enum",
    "Decorator",
    "Import",
    "Type",
    "CodeElement",
    "Community",
    "Process",
    "Struct",
    "Namespace",
    "Trait",
    "Constructor",
];

/// Common relationship types GitNexus emits (not exhaustive).
pub const RELATIONSHIP_TYPES: &[&str] = &[
    "CONTAINS",
    "CALLS",
    "INHERITS",
    "IMPORTS",
    "USES",
    "DEFINES",
    "DECORATES",
    "IMPLEMENTS",
    "EXTENDS",
    "HAS_METHOD",
    "HAS_PROPERTY",
    "ACCESSES",
    "MEMBER_OF",
    "METHOD_OVERRIDES",
    "METHOD_IMPLEMENTS",
    "STEP_IN_PROCESS",
];

/// A node in the GitNexus knowledge graph.
#[derive(Debug, Clone)]
pub struct GraphNode {
    pub id: String,
    pub label: String,
    pub properties: HashMap<String, Value>,
}

/// An edge in the GitNexus knowledge graph.
#[derive(Debug, Clone)]
pub struct GraphRelationship {
    pub id: String,
    pub source_id: String,
    pub target_id: String,
    pub rel_type: String,
    pub confidence: f64,
    pub reason: String,
    /// Arbitrary GitNexus payload (e.g. `STEP_IN_PROCESS` edges carry a
    /// `step` ordinal that [`crate::graphs::process`] sorts by).
    pub properties: HashMap<String, Value>,
}

/// Parse one legacy-JSON-format node record.
pub fn parse_node(item: &Value) -> Option<GraphNode> {
    Some(GraphNode {
        id: item.get("id")?.as_str()?.to_string(),
        label: item.get("label")?.as_str()?.to_string(),
        properties: item
            .get("properties")
            .and_then(Value::as_object)
            .map(|obj| obj.clone().into_iter().collect())
            .unwrap_or_default(),
    })
}

/// Parse one legacy-JSON-format relationship record.
pub fn parse_relationship(item: &Value) -> Option<GraphRelationship> {
    let source_id = item.get("sourceId")?.as_str()?.to_string();
    let target_id = item.get("targetId")?.as_str()?.to_string();
    let id = item
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| format!("{source_id}->{target_id}"));
    Some(GraphRelationship {
        id,
        source_id,
        target_id,
        rel_type: item.get("type")?.as_str()?.to_string(),
        confidence: item
            .get("confidence")
            .and_then(Value::as_f64)
            .unwrap_or(1.0),
        reason: item
            .get("reason")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        properties: item
            .get("properties")
            .and_then(Value::as_object)
            .map(|obj| obj.clone().into_iter().collect())
            .unwrap_or_default(),
    })
}
