//! UAST mapper common — shared logic for mapping tree-sitter concrete
//! syntax trees (CSTs) to the normalized UAST representation.
//!
//! This module provides the core transformation engine that:
//!
//! 1. **Filters noise**: only "named" nodes from tree-sitter are mapped;
//!    anonymous nodes (punctuation, keywords, whitespace) are ignored.
//! 2. **Standardizes kinds**: uses language-specific mapping functions to
//!    translate native tree-sitter types into unified UAST kinds.
//! 3. **Preserves fidelity**: populates every [`UASTNode`] with the
//!    original byte spans and a [`NativeRef`] containing the parser
//!    identity and native node type.
//! 4. **Excludes test-only nodes**: language mappers may supply an
//!    [`TestNodeFilter`] so test-only constructs (e.g. Rust
//!    `#[cfg(test)]` items, Python `if __name__ == "__main__":` guards)
//!    are dropped from the SIMPLE-relevant AST without this shared engine
//!    needing any language-specific knowledge.
//! 5. **Attaches language attributes**: language mappers may supply
//!    `extract_attributes` to add normalized metadata such as `typeKind`.

use std::collections::{HashMap, HashSet};

use blake2::digest::{Update, VariableOutput};
use blake2::Blake2bVar;
use tree_sitter::Node;

use super::models::{AttributeValue, NativeRef, SourceSpan, UASTNode};

/// Per-language attribute extractor -- see [`map_tree_sitter_to_uast`]'s
/// `extract_attributes` parameter.
pub type AttributeExtractor<'a> = dyn Fn(&Node, &[u8]) -> HashMap<String, AttributeValue> + 'a;

/// A per-language classifier deciding which of a node's named siblings
/// are test-only scaffolding that should be excluded from the UAST
/// (along with their whole subtree).
///
/// `drop_set` sees the full ordered list of named siblings of one parent
/// — i.e. exactly what that parent's named children are before any
/// filtering — and returns the [`tree_sitter::Node::id`]s to drop.
///
/// This is a *batch* classifier over the whole sibling list, not a
/// per-node predicate, because some languages need positional/stateful
/// context to classify a single node: Rust expresses "this is test code"
/// as a *separate preceding sibling* attribute rather than as part of
/// the node it applies to, so answering "is this node dropped?" requires
/// knowing what came immediately before it in the sibling list. A
/// per-node query interface would force that scan to be repeated from
/// scratch for every sibling (`O(n)` work × `n` nodes = `O(n²)` per
/// parent); a single pass over the whole list computes the same
/// classification in `O(n)`. Languages whose test markers are
/// self-contained within one node (e.g. Python's
/// `if __name__ == "__main__":` guard) still do a single `O(n)` pass,
/// just without needing any cross-node state.
///
/// `source` is the original source bytes — Rust's tree-sitter bindings,
/// unlike Python's, don't cache the source on the node, so any
/// implementation that needs a node's text (both `RustCfgTestFilter` and
/// `PythonMainGuardFilter` do) must slice it explicitly via
/// [`tree_sitter::Node::utf8_text`].
///
/// Each language mapper owns its own filter and passes it to
/// [`map_tree_sitter_to_uast`] via `is_test_node`. Languages that don't
/// (yet) filter test nodes pass `None`, which preserves "map every named
/// node" behavior.
pub trait TestNodeFilter {
    fn drop_set(&self, named_siblings: &[Node], source: &[u8]) -> HashSet<usize>;
}

const LOGICAL_OPERATORS: &[&str] = &["and", "or", "&&", "||"];

fn is_logical_operator_text(text: &str) -> bool {
    LOGICAL_OPERATORS.contains(&text)
}

fn logical_operator_from_node(node: &Node, source: &[u8]) -> Option<String> {
    match node.kind() {
        "and" | "or" => Some(node.kind().to_string()),
        _ => node.utf8_text(source).ok().and_then(|text| {
            let text = text.trim();
            if is_logical_operator_text(text) {
                Some(text.to_string())
            } else {
                None
            }
        }),
    }
}

fn extract_binary_logical_operator(node: &Node, source: &[u8]) -> Option<String> {
    if let Some(op_node) = node.child_by_field_name("operator") {
        if let Some(op) = logical_operator_from_node(&op_node, source) {
            return Some(op);
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(op) = logical_operator_from_node(&child, source) {
            return Some(op);
        }
    }
    None
}

fn extract_boolean_operator_logical_op(node: &Node, source: &[u8]) -> Option<String> {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if matches!(child.kind(), "and" | "or") {
            return Some(child.kind().to_string());
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(op) = logical_operator_from_node(&child, source) {
            return Some(op);
        }
    }
    None
}

/// When mapping `boolean_operator` / `binary_expression` to `BinaryExpr`,
/// record short-circuit logical operator text for `ast.max_function_complexity`
/// (issue #142). Nullish coalescing (`??`) is intentionally excluded.
pub fn logical_operator_attribute(node: &Node, source: &[u8]) -> HashMap<String, AttributeValue> {
    let op = match node.kind() {
        "boolean_operator" => extract_boolean_operator_logical_op(node, source),
        "binary_expression" => extract_binary_logical_operator(node, source),
        _ => None,
    };
    match op {
        Some(text) if is_logical_operator_text(&text) => {
            HashMap::from([("operator".to_string(), AttributeValue::Str(text))])
        }
        _ => HashMap::new(),
    }
}

const TREE_SITTER_PACKAGES: &[(&str, &str)] = &[
    ("python", "tree-sitter-python"),
    ("rust", "tree-sitter-rust"),
    ("javascript", "tree-sitter-javascript"),
    ("typescript", "tree-sitter-typescript"),
    ("cpp", "tree-sitter-cpp"),
    ("go", "tree-sitter-go"),
];

/// Return the canonical `(parser_name, parser_version)` for a language.
///
/// Unknown languages fall back to `("tree-sitter", "unknown")`.
///
/// # Deviation from the Python original
///
/// Python's `parser_identity` calls `importlib.metadata.version()` to
/// read the *actually installed* grammar package's version at runtime,
/// and has a `native=True` mode returning CPython's own `ast` module
/// identity. Rust has no equivalent runtime package-version
/// introspection, and no "native provider" exists on this side (see
/// [`crate::graphs::ast::dispatch`] for why one isn't needed) — so this
/// always returns `"unknown"` for the version.
///
/// ponytail: wire the real grammar crate version through from
/// `Cargo.lock` (e.g. via `build.rs`) if a caller ever needs it; nothing
/// currently reads `parser_version` for more than display purposes.
pub fn parser_identity(language: &str) -> (&'static str, &'static str) {
    match TREE_SITTER_PACKAGES
        .iter()
        .find(|(lang, _)| *lang == language)
    {
        Some((_, package)) => (package, "unknown"),
        None => ("tree-sitter", "unknown"),
    }
}

/// Compute a deterministic 16-hex-char node id: BLAKE2b (8-byte digest)
/// of `lang|node_kind|start_byte|end_byte|parent_id`.
///
/// Matches Python's `hashlib.blake2b(payload, digest_size=8).hexdigest()`
/// byte-for-byte — see the parity test below, checked against real
/// `hashlib` output — since this id crosses the Rust/Python boundary
/// today (both sides may compute it for the same source) and is meant to
/// remain a stable cross-tool reference per [`UASTNode::id`]'s contract.
fn compute_node_id(
    language: &str,
    node_kind: &str,
    start_byte: usize,
    end_byte: usize,
    parent_id: &str,
) -> String {
    let payload = format!("{language}|{node_kind}|{start_byte}|{end_byte}|{parent_id}");
    let mut hasher = Blake2bVar::new(8).expect("8 is a valid BLAKE2b-var digest size");
    hasher.update(payload.as_bytes());
    let mut digest = [0u8; 8];
    hasher
        .finalize_variable(&mut digest)
        .expect("digest buffer matches the requested output size");
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Named children of `node`, minus any the language's `is_test_node`
/// filter flags as test-only.
///
/// The filter sees the full named-sibling list in one pass (not a single
/// candidate node queried repeatedly) so languages whose test markers
/// live on a *separate* sibling — e.g. Rust's `#[cfg(test)]` attribute
/// preceding the item it annotates — can correlate adjacent siblings in
/// `O(n)` instead of re-scanning per candidate.
fn filtered_named_children<'a>(
    node: Node<'a>,
    is_test_node: Option<&dyn TestNodeFilter>,
    source: &[u8],
) -> Vec<Node<'a>> {
    let mut cursor = node.walk();
    let named: Vec<Node<'a>> = node.named_children(&mut cursor).collect();
    match is_test_node {
        None => named,
        Some(filter) => {
            let dropped = filter.drop_set(&named, source);
            named
                .into_iter()
                .filter(|c| !dropped.contains(&c.id()))
                .collect()
        }
    }
}

/// Map a tree-sitter CST to the normalized UAST representation.
///
/// `is_test_node`, when provided, classifies each node's named siblings
/// in one pass to decide which are test-only scaffolding that should be
/// excluded from the SIMPLE-relevant AST — see [`TestNodeFilter`].
/// Languages that don't provide one keep the default behavior of mapping
/// every named node.
///
/// `source` must be the exact byte buffer `root`'s tree was parsed from
/// (needed both for `is_test_node` implementations and is otherwise
/// unused by this function itself).
///
/// Uses a two-phase *iterative* traversal — avoiding recursion limits on
/// deeply nested trees (macro-expanded Rust, minified JS, etc.), matching
/// the Python original: phase 1 is a pre-order DFS to record visit order
/// and compute stable ids; phase 2 walks that order in reverse (children
/// before parents) to build [`UASTNode`]s bottom-up. Both phases
/// recompute `filtered_named_children` per node — a small duplicated cost
/// the Python original also pays, kept here for behavioral parity rather
/// than optimized away.
pub fn map_tree_sitter_to_uast(
    root: Node,
    language: &str,
    map_node_kind: impl Fn(&str) -> &'static str,
    source: &[u8],
    file: Option<&str>,
    is_test_node: Option<&dyn TestNodeFilter>,
    extract_attributes: Option<&AttributeExtractor>,
) -> UASTNode {
    let (parser_name, parser_version) = parser_identity(language);

    let mut order: Vec<(Node, String)> = Vec::new();
    let mut stack: Vec<(Node, String)> = vec![(root, String::new())];
    while let Some((node, parent_stable_id)) = stack.pop() {
        let node_stable_id = compute_node_id(
            language,
            node.kind(),
            node.start_byte(),
            node.end_byte(),
            &parent_stable_id,
        );
        for child in filtered_named_children(node, is_test_node, source)
            .into_iter()
            .rev()
        {
            stack.push((child, node_stable_id.clone()));
        }
        order.push((node, node_stable_id));
    }

    let mut uast_nodes: HashMap<usize, UASTNode> = HashMap::new();
    for (node, node_stable_id) in order.into_iter().rev() {
        let named_children = filtered_named_children(node, is_test_node, source);
        let children: Vec<UASTNode> = named_children
            .iter()
            .map(|c| {
                uast_nodes
                    .remove(&c.id())
                    .expect("children are built before their parent in reverse pre-order")
            })
            .collect();

        let start = node.start_position();
        let end = node.end_position();
        let mut attributes = HashMap::new();
        attributes.insert("named".to_string(), AttributeValue::Bool(node.is_named()));
        if let Some(extract) = extract_attributes {
            attributes.extend(extract(&node, source));
        }

        uast_nodes.insert(
            node.id(),
            UASTNode {
                kind: map_node_kind(node.kind()).to_string(),
                lang: language.to_string(),
                span: SourceSpan {
                    file: file.map(str::to_string),
                    start_byte: node.start_byte(),
                    end_byte: node.end_byte(),
                    start_line: start.row + 1,
                    start_column: start.column,
                    end_line: end.row + 1,
                    end_column: end.column,
                },
                native: NativeRef {
                    parser: parser_name.to_string(),
                    parser_version: parser_version.to_string(),
                    node_kind: node.kind().to_string(),
                },
                attributes,
                children,
                id: node_stable_id,
            },
        );
    }

    uast_nodes
        .remove(&root.id())
        .expect("the root is always built in phase 2")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reference values from Python: `hashlib.blake2b(payload.encode(),
    /// digest_size=8).hexdigest()` — verified by actually running
    /// `hashlib` locally, not derived from the algorithm's spec alone.
    #[test]
    fn compute_node_id_matches_python_blake2b() {
        assert_eq!(
            compute_node_id("python", "module", 0, 10, ""),
            "b3b85bef58cfada6"
        );
        assert_eq!(
            compute_node_id("rust", "source_file", 5, 42, "abc123"),
            "da05e40ef78981b5"
        );
    }

    #[test]
    fn parser_identity_known_and_unknown() {
        assert_eq!(parser_identity("python"), ("tree-sitter-python", "unknown"));
        assert_eq!(parser_identity("cobol"), ("tree-sitter", "unknown"));
    }
}
