//! UAST models — the "Normalized" layer of Topos's "native-first,
//! normalized-second" architecture.
//!
//! Data structures for the Universal Abstract Syntax Tree: a
//! language-neutral tree that every `graphs::uast::mapper_*` module
//! produces from a language-specific tree-sitter CST, and that every
//! downstream structural probe (CFG/CPG/PDG builders, issue #143)
//! consumes uniformly regardless of source language.

use std::collections::HashMap;

/// A byte/line/column range in a source file.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SourceSpan {
    pub file: Option<String>,
    pub start_byte: usize,
    pub end_byte: usize,
    pub start_line: usize,
    pub start_column: usize,
    pub end_line: usize,
    pub end_column: usize,
}

/// Provenance of the parser that produced a native node.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct NativeRef {
    pub parser: String,
    pub parser_version: String,
    pub node_kind: String,
}

/// A UAST node attribute value.
///
/// Narrows Python's `dict[str, Any]` — the concrete uses seen so far are
/// booleans like `mapper_common`'s `"named"` and strings like the
/// mappers' `"operator"` / `"typeKind"`. Widen with another variant if a
/// future attribute needs a richer value.
#[derive(Debug, Clone, PartialEq)]
pub enum AttributeValue {
    Bool(bool),
    Str(String),
}

/// Language-normalized node carrying provenance and source spans.
///
/// `UASTNode` acts as a normalization layer over language-specific
/// concrete syntax trees (CSTs) from tree-sitter. It maps disparate
/// native nodes into unified `kind` values that follow the
/// industry-standard reference in `docs/decisions/uast-industry-standards.md`.
///
/// While normalized, each node strictly retains its `native` provenance
/// and `span` data to ensure fidelity with compiler-native AST
/// expectations (e.g. Python `ast`, ESTree, Rust `syn`, Clang).
///
/// `id` is a deterministic 16-hex-char identifier: a BLAKE2b-8-byte hash
/// of `(lang, native.node_kind, span.start_byte, span.end_byte,
/// parent_id)` (see `mapper_common::compute_node_id`). Chaining the
/// parent's id encodes the full path from the root, which disambiguates
/// identical-span sibling nodes without needing an explicit sibling
/// index. The mapper walker populates it; a node built directly (e.g. in
/// tests) with no id supplied defaults to the empty string.
///
/// `Clone`, `Drop`, and `PartialEq` are all implemented iteratively so
/// pathologically deep trees can't overflow the stack. `Debug` remains
/// derived (recursive): it is a diagnostic aid never applied to
/// untrusted input, so avoid formatting extremely deep trees.
#[derive(Debug)]
pub struct UASTNode {
    pub kind: String,
    pub lang: String,
    pub span: SourceSpan,
    pub native: NativeRef,
    pub attributes: HashMap<String, AttributeValue>,
    pub children: Vec<UASTNode>,
    pub id: String,
}

impl Clone for UASTNode {
    fn clone(&self) -> Self {
        let mut order = Vec::new();
        let mut stack = vec![self];
        while let Some(node) = stack.pop() {
            order.push(node);
            stack.extend(node.children.iter());
        }

        let mut cloned = HashMap::with_capacity(order.len());
        for node in order.into_iter().rev() {
            let children = node
                .children
                .iter()
                .map(|child| {
                    cloned
                        .remove(&std::ptr::from_ref(child))
                        .expect("children are cloned before their parent")
                })
                .collect();
            cloned.insert(
                std::ptr::from_ref(node),
                UASTNode {
                    kind: node.kind.clone(),
                    lang: node.lang.clone(),
                    span: node.span.clone(),
                    native: node.native.clone(),
                    attributes: node.attributes.clone(),
                    children,
                    id: node.id.clone(),
                },
            );
        }

        cloned
            .remove(&std::ptr::from_ref(self))
            .expect("the root is cloned in the final pass")
    }
}

impl PartialEq for UASTNode {
    fn eq(&self, other: &Self) -> bool {
        let mut stack = vec![(self, other)];
        while let Some((a, b)) = stack.pop() {
            if a.kind != b.kind
                || a.lang != b.lang
                || a.span != b.span
                || a.native != b.native
                || a.attributes != b.attributes
                || a.id != b.id
                || a.children.len() != b.children.len()
            {
                return false;
            }
            stack.extend(a.children.iter().zip(b.children.iter()));
        }
        true
    }
}

impl Drop for UASTNode {
    fn drop(&mut self) {
        let mut descendants = std::mem::take(&mut self.children);
        while let Some(mut node) = descendants.pop() {
            descendants.append(&mut node.children);
        }
    }
}

/// Identity key for a node within a single build: the mapper-assigned
/// [`UASTNode::id`] when present, or `anon::{address:x}` from the node's
/// location in memory otherwise.
///
/// Every layer that cross-references nodes between graph representations
/// (CFG block statements, the CPG node map, PDG dependence edges) must
/// key nodes through this one helper so an anonymous (empty-id) node
/// resolves to the same key everywhere. Mirrors Python's
/// `node.id or f"anon::{id(node):x}"`, where `id()` is object identity —
/// the node's address is Rust's closest equivalent.
///
/// The pointer fallback is only meaningful while the borrowed tree is
/// alive, within a single build of one tree: addresses are not stable
/// across separate parses, so anonymous keys must never be compared
/// between trees. Mapper-produced nodes always carry a real id, which is
/// what keeps cross-tree comparisons (e.g. CPG node Jaccard) sound.
pub(crate) fn node_key(node: &UASTNode) -> String {
    if node.id.is_empty() {
        format!("anon::{:x}", std::ptr::from_ref(node) as usize)
    } else {
        node.id.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_node(kind: &str) -> UASTNode {
        UASTNode {
            kind: kind.to_string(),
            lang: "python".to_string(),
            span: SourceSpan {
                file: None,
                start_byte: 0,
                end_byte: 0,
                start_line: 0,
                start_column: 0,
                end_line: 0,
                end_column: 0,
            },
            native: NativeRef {
                parser: "test".to_string(),
                parser_version: "0".to_string(),
                node_kind: kind.to_lowercase(),
            },
            attributes: HashMap::new(),
            children: Vec::new(),
            id: String::new(),
        }
    }

    fn deep_chain(depth: usize) -> UASTNode {
        let mut node = test_node("Leaf");
        for _ in 0..depth {
            let mut parent = test_node("Wrap");
            parent.children.push(node);
            node = parent;
        }
        node
    }

    #[test]
    fn node_key_prefers_the_mapper_id() {
        let mut node = test_node("Stmt");
        node.id = "deadbeefdeadbeef".to_string();
        assert_eq!(node_key(&node), "deadbeefdeadbeef");
    }

    #[test]
    fn node_key_falls_back_to_anon_ptr_for_empty_id_nodes() {
        let anon = test_node("Stmt");
        let key = node_key(&anon);
        assert!(
            key.starts_with("anon::"),
            "expected `anon::<ptr>` key for empty-id node, got {key:?}"
        );
        assert_eq!(key, node_key(&anon), "key must be stable for one node");
        assert_ne!(
            node_key(&test_node("Stmt")),
            key,
            "distinct nodes must get distinct keys"
        );
    }

    #[test]
    fn eq_is_stack_safe_on_deeply_nested_trees() {
        const DEPTH: usize = 100_000;
        let original = deep_chain(DEPTH);
        let mut copy = original.clone();
        assert!(original == copy);

        let mut cursor = &mut copy;
        while !cursor.children.is_empty() {
            cursor = &mut cursor.children[0];
        }
        cursor.kind = "Changed".to_string();
        assert!(original != copy);
    }
}
