//! `CodePropertyGraph` — implements [`Representation`] on the SECURE
//! generator.

use std::collections::HashMap;

use super::builder::build_cpg;
use super::models::{CPGEdge, CPGNode};
use crate::graphs::base::Representation;
use crate::graphs::uast::models::UASTNode;

/// A Code Property Graph (Yamaguchi et al., arxiv:1909.03496).
#[derive(Debug, Clone, Default)]
pub struct CodePropertyGraph {
    /// UAST nodes keyed by stable id.
    pub nodes: HashMap<String, CPGNode>,
    /// Labeled CPG edges across the four families {AST, CFG, DDG, CDG}.
    pub edges: Vec<CPGEdge>,
    /// The source language (passed through for danger-registry lookup).
    pub language: String,
    /// Original source text — needed to recover token text from spans.
    pub source: String,
}

impl CodePropertyGraph {
    pub fn from_uast(uast_root: &UASTNode, source: impl Into<String>) -> Self {
        let (nodes, edges) = build_cpg(uast_root);
        CodePropertyGraph {
            nodes,
            edges,
            language: uast_root.lang.clone(),
            source: source.into(),
        }
    }

    /// Slice the original source by a node's byte span.
    pub fn node_text(&self, node: &CPGNode) -> String {
        if self.source.is_empty() {
            return String::new();
        }
        let span = &node.uast.span;
        let bytes = self.source.as_bytes();
        // Defensive bounds — source may be a different revision than the parse.
        if span.end_byte > bytes.len() {
            return String::new();
        }
        String::from_utf8_lossy(&bytes[span.start_byte..span.end_byte]).into_owned()
    }

    // --- Queries used by security probes ---------------------------------

    pub fn nodes_of_kind(&self, kind: &str) -> Vec<&CPGNode> {
        self.nodes.values().filter(|n| n.kind() == kind).collect()
    }
}

impl Representation for CodePropertyGraph {
    fn name(&self) -> &str {
        "cpg"
    }

    fn dimension(&self) -> &str {
        "secure"
    }

    fn metrics(&self) -> HashMap<String, f64> {
        unimplemented!(
            "CodePropertyGraph::metrics depends on functors::probes::cpg::{{danger, taint}} (issue #145)"
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphs::ast::dispatch::parse_source;

    #[test]
    fn from_uast_builds_nodes_and_node_text_recovers_source() {
        let source = "x = 1\n";
        let result = parse_source(source, "python", None).unwrap();
        let cpg = CodePropertyGraph::from_uast(&result.uast_root, source);
        assert!(!cpg.nodes.is_empty());
        assert_eq!(cpg.language, "python");

        let root_node = cpg.nodes.get(&result.uast_root.id).unwrap();
        assert_eq!(cpg.node_text(root_node), source);
    }

    #[test]
    fn node_text_is_empty_when_no_source_stored() {
        let source = "x = 1\n";
        let result = parse_source(source, "python", None).unwrap();
        let cpg = CodePropertyGraph::from_uast(&result.uast_root, "");
        let root_node = cpg.nodes.get(&result.uast_root.id).unwrap();
        assert_eq!(cpg.node_text(root_node), "");
    }
}
