//! Parse artifacts shared across the AST layer.

use tree_sitter::Tree;

use crate::graphs::uast::models::UASTNode;

/// Metadata describing how a tree was produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParserProvenance {
    pub parser: String,
    pub parser_version: String,
    pub node_kind: String,
}

/// Container for a language's parse artifacts.
///
/// Holds the owning [`Tree`] rather than a bare `Node` for the same
/// reason [`crate::core::object::ProgramObject`] does — see that type's
/// doc comment. `uast_root` is always populated: unlike the Python
/// original (whose `native_ast`/`uast_root` are `Any | None` because a
/// "native" backend could in principle skip UAST construction), this
/// crate has exactly one parsing path (tree-sitter → UAST), and no
/// `native_ast` field at all — see [`super::dispatch`] for why.
pub struct ParseResult {
    pub tree: Tree,
    pub source: String,
    pub language: String,
    pub provenance: ParserProvenance,
    pub uast_root: UASTNode,
    pub has_errors: bool,
}

impl ParseResult {
    pub fn root(&self) -> tree_sitter::Node<'_> {
        self.tree.root_node()
    }
}
