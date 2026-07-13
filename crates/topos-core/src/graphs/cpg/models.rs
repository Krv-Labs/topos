//! CPG models — Code Property Graph node & edge types per Yamaguchi et
//! al. (arxiv:1909.03496).

use std::collections::HashMap;

use crate::graphs::uast::models::{AttributeValue, UASTNode};

/// The four CPG edge families.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CPGEdgeKind {
    /// parent → child
    Ast,
    /// control-flow successor
    Cfg,
    /// data dependence (def → use)
    Ddg,
    /// control dependence (predicate → executor)
    Cdg,
}

/// A typed, labeled edge in the CPG multigraph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CPGEdge {
    /// Source node id (UAST node id).
    pub source: String,
    pub target: String,
    pub kind: CPGEdgeKind,
    /// Variable name for DDG, branch label for CFG, ...
    pub label: String,
}

impl CPGEdge {
    pub fn new(
        source: impl Into<String>,
        target: impl Into<String>,
        kind: CPGEdgeKind,
        label: impl Into<String>,
    ) -> Self {
        CPGEdge {
            source: source.into(),
            target: target.into(),
            kind,
            label: label.into(),
        }
    }
}

/// A CPG node: a UAST node enriched with quick-lookup metadata.
///
/// The CPG uses the UAST node directly as the node payload — every CPG
/// node *is* a UAST node, so the AST family of edges is implicit in the
/// UAST `children` lists. We materialize them as `CPGEdge`s anyway so
/// downstream queries are uniform.
#[derive(Debug, Clone)]
pub struct CPGNode {
    pub uast: UASTNode,
}

impl CPGNode {
    pub fn id(&self) -> &str {
        &self.uast.id
    }

    pub fn kind(&self) -> &str {
        &self.uast.kind
    }

    pub fn attributes(&self) -> &HashMap<String, AttributeValue> {
        &self.uast.attributes
    }
}
