//! `Object` — the AST lifted into the category of programs.
//!
//! In category theory, an "object" is an abstract entity that serves as
//! the domain or codomain of morphisms. In the category of programs, we
//! model the Abstract Syntax Tree (AST) as our primary object.
//!
//! # Mathematical inspiration
//!
//! An object in our category represents the "shape" or "structure" of a
//! computation — not what the program does, but how it is organized. Two
//! programs with isomorphic ASTs are considered structurally equivalent,
//! even if their surface syntax differs.
//!
//! This abstraction allows us to reason about code structurally:
//! transformations (refactors) that preserve the AST structure are
//! isomorphisms in the category of programs.

use std::cell::Cell;
use tree_sitter::{Node, Tree};

use crate::graphs::uast::models::UASTNode;

/// The AST lifted into the category of programs.
///
/// A `ProgramObject` wraps a parsed tree-sitter AST and provides methods
/// for structural analysis. It represents the "shape" of code — the
/// invariant structure that remains after stripping away surface syntax.
///
/// # Categorical interpretation
///
/// Objects in our category are ASTs. A morphism `f: A → B` represents a
/// program that transforms computations of shape `A` into shape `B`.
///
/// # Deviation from the Python original
///
/// The Python `ProgramObject` stores a bare `root: Node` — tree-sitter's
/// Python bindings keep the owning tree alive via Python's own object
/// graph, so a lone `Node` is safe to hold there. Rust has no such GC to
/// lean on: a [`tree_sitter::Node`] borrows from the [`tree_sitter::Tree`]
/// that produced it, so this struct owns the `Tree` and hands out a
/// `Node` on demand via [`ProgramObject::root`] instead of storing one.
pub struct ProgramObject {
    tree: Tree,
    /// The original source code (for reference).
    pub source: String,
    /// The programming language of the source.
    pub language: String,
    /// Parser identifier (e.g. `"tree-sitter"`).
    pub parser_name: String,
    /// Parser/grammar version string.
    pub parser_version: String,
    /// The tree-sitter node kind of the root (e.g. `"module"`).
    pub native_node_kind: String,
    /// The language-neutral UAST tree for this source — `Any` on the
    /// Python side (deferred here until `graphs::uast` landed in issue
    /// #142; now a concrete type).
    pub uast_root: UASTNode,
    node_count: Cell<Option<usize>>,
    // `native_ast` (Python's optional CPython `ast.Module`) has no Rust
    // equivalent and isn't ported — see `graphs::ast::dispatch`'s doc
    // comment for why there's no "native provider" on this side.
}

impl ProgramObject {
    /// Wrap an already-parsed tree with its source metadata and UAST.
    pub fn new(
        tree: Tree,
        source: impl Into<String>,
        language: impl Into<String>,
        uast_root: UASTNode,
    ) -> Self {
        ProgramObject {
            tree,
            source: source.into(),
            language: language.into(),
            parser_name: "tree-sitter".to_string(),
            parser_version: "tree-sitter>=0.23".to_string(),
            native_node_kind: "module".to_string(),
            uast_root,
            node_count: Cell::new(None),
        }
    }

    /// The root node of the parsed AST.
    pub fn root(&self) -> Node<'_> {
        self.tree.root_node()
    }

    /// Total number of nodes in the AST (cached after first computation).
    pub fn node_count(&self) -> usize {
        if let Some(count) = self.node_count.get() {
            return count;
        }
        let count = Self::count_nodes(self.root());
        self.node_count.set(Some(count));
        count
    }

    /// Maximum depth of the AST.
    pub fn depth(&self) -> usize {
        Self::calculate_depth(self.root(), 0)
    }

    /// Whether the AST has no syntax errors.
    pub fn is_valid(&self) -> bool {
        !self.root().has_error()
    }

    /// Depth-first traversal of all nodes.
    ///
    /// ponytail: the Python original yields lazily; every call site here
    /// (`node_count`, `nodes_of_type`) already consumes the full
    /// traversal, so this collects eagerly instead of hand-rolling a
    /// cursor-based `Iterator`. Revisit if a caller ever needs to
    /// short-circuit over a very large tree.
    pub fn traverse(&self) -> Vec<Node<'_>> {
        let mut nodes = Vec::new();
        Self::traverse_node(self.root(), &mut nodes);
        nodes
    }

    /// Find all nodes whose kind matches any of `kinds`.
    pub fn nodes_of_type(&self, kinds: &[&str]) -> Vec<Node<'_>> {
        self.traverse()
            .into_iter()
            .filter(|node| kinds.contains(&node.kind()))
            .collect()
    }

    // These three walk `node`'s children via tree-sitter's cursor-based
    // `children()` iterator rather than manual `child_count()`/`child(i)`
    // indexing — dogfooding `topos evaluate` on this file during the
    // migration flagged the indexed version for cyclomatic complexity
    // (21, exceeding the SIMPLE threshold of 15); the cursor iterator
    // yields nodes directly (no `Option` to unwrap), which removes a
    // branch per recursive call on top of being the idiomatic API.

    fn traverse_node<'a>(node: Node<'a>, out: &mut Vec<Node<'a>>) {
        out.push(node);
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            Self::traverse_node(child, out);
        }
    }

    fn count_nodes(node: Node<'_>) -> usize {
        let mut cursor = node.walk();
        1 + node
            .children(&mut cursor)
            .map(Self::count_nodes)
            .sum::<usize>()
    }

    fn calculate_depth(node: Node<'_>, current: usize) -> usize {
        let mut cursor = node.walk();
        node.children(&mut cursor)
            .map(|child| Self::calculate_depth(child, current + 1))
            .max()
            .unwrap_or(current)
    }
}

impl PartialEq for ProgramObject {
    /// Structural equality based on source + language, matching the
    /// Python original — this deliberately does *not* compare parsed
    /// trees; two objects with the same source are equal regardless of
    /// how many times each was independently parsed.
    fn eq(&self, other: &Self) -> bool {
        self.source == other.source && self.language == other.language
    }
}

impl Eq for ProgramObject {}

impl std::hash::Hash for ProgramObject {
    /// Hashes on source only, matching the Python original. `eq` also
    /// checks `language` — a weaker-than-necessary but still
    /// contract-compliant combination (equal objects still hash equal;
    /// Rust's `Hash`/`Eq` contract only requires that direction).
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.source.hash(state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphs::ast::dispatch::parse_source;

    fn build(source: &str) -> ProgramObject {
        let result = parse_source(source, "python", None).expect("parse should not fail");
        ProgramObject::new(
            result.tree,
            result.source,
            result.language,
            result.uast_root,
        )
    }

    #[test]
    fn program_object_basic() {
        let source = "def hello():\n    print('world')";
        let obj = build(source);

        assert_eq!(obj.source, source);
        assert_eq!(obj.language, "python");
        assert!(obj.is_valid());
        assert!(obj.node_count() > 0);
        assert!(obj.depth() > 0);
    }

    #[test]
    fn program_object_traversal() {
        let obj = build("x = 1 + 2");

        let nodes = obj.traverse();
        assert_eq!(nodes.len(), obj.node_count());

        let assignments = obj.nodes_of_type(&["assignment"]);
        assert_eq!(assignments.len(), 1);
    }

    #[test]
    fn program_object_invalid_syntax() {
        let obj = build("def incomplete_func(");
        assert!(!obj.is_valid());
    }

    #[test]
    fn program_object_equality() {
        let a = build("x = 1");
        let b = build("x = 1");
        let c = build("y = 2");
        assert!(a == b);
        assert!(a != c);
    }
}
