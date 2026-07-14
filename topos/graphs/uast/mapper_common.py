"""
UAST Mapper Common
------------------

Shared logic for mapping Tree-sitter Concrete Syntax Trees (CSTs) to the
normalized UAST representation.

This module provides the core transformation engine that:

1.  **Filters Noise**: Only "named" nodes from Tree-sitter are mapped; anonymous
    nodes (punctuation, keywords, whitespace) are ignored.
2.  **Standardizes Kinds**: Uses language-specific mapping functions to translate
    native Tree-sitter types into unified UNodeKinds.
3.  **Preserves Fidelity**: Populates every UASTNode with the original byte
    spans and a NativeRef containing the parser identity and native node type.
4.  **Excludes Test-Only Nodes**: Language mappers may supply an
    `is_test_node` predicate (see `TestNodeFilter`) so test-only
    constructs (e.g. Rust `#[cfg(test)]` modules, Python
    `if __name__ == "__main__":` guards) are dropped from the SIMPLE-relevant
    AST without the shared engine needing any language-specific knowledge.
5.  **Attaches Language Attributes**: Language mappers may supply
    `extract_attributes` to add normalized metadata such as `typeKind`.
"""

from __future__ import annotations

import hashlib
import sys
from collections.abc import Callable
from importlib.metadata import PackageNotFoundError, version

from tree_sitter import Node

from topos.graphs.uast.models import NativeRef, SourceSpan, UASTNode

# A per-language classifier deciding which of a node's named siblings are
# test-only scaffolding that should be excluded from the UAST (along with
# their whole subtrees).
#
# Signature: `(named_siblings) -> {id, ...}`, where `named_siblings` is the
# full ordered list of named children of one parent — i.e. exactly what
# `node.parent`'s named children are before any filtering — and the return
# value is the set of `Node.id`s (tree-sitter's stable per-node identity)
# to drop.
#
# This is a *batch* classifier, not a per-node predicate, because some
# languages need positional/stateful context to classify a single node: Rust
# expresses "this is test code" as a *separate preceding sibling* attribute
# rather than as part of the node it applies to, so answering "is this node
# dropped?" requires knowing what came immediately before it in the sibling
# list. A per-node query interface would force that scan to be repeated from
# scratch for every sibling (O(n) work × n nodes = O(n²) per parent); a
# single pass over the whole list computes the same classification in O(n).
# Languages whose test markers are self-contained within one node (e.g.
# Python's `if __name__ == "__main__":` guard) still do a single O(n) pass,
# just without needing any cross-node state.
#
# Each language mapper owns its own classifier and passes it to
# `map_tree_sitter_to_uast` via `is_test_node`, the same way `map_node_kind`
# is threaded through today. Languages that don't (yet) filter test nodes
# pass `None` (the default), which preserves today's "no filtering" behavior.
TestNodeFilter = Callable[[list[Node]], set[int]]

_TREE_SITTER_PACKAGE = {
    "python": "tree-sitter-python",
    "rust": "tree-sitter-rust",
    "javascript": "tree-sitter-javascript",
    "typescript": "tree-sitter-typescript",
    "cpp": "tree-sitter-cpp",
    "go": "tree-sitter-go",
}


def parser_identity(language: str, *, native: bool = False) -> tuple[str, str]:
    """
    Return the canonical `(parser_name, parser_version)` for a language.

    `native=True` selects the language's native parser identity (currently
    only CPython's stdlib `ast` for Python); otherwise the tree-sitter
    grammar identity is returned. Unknown languages fall back to
    `("tree-sitter", "unknown")`.
    """
    if native and language == "python":
        return (
            "cpython-ast",
            f"python-{sys.version_info.major}.{sys.version_info.minor}",
        )

    package = _TREE_SITTER_PACKAGE.get(language)
    if package is None:
        return "tree-sitter", "unknown"
    try:
        return package, version(package)
    except PackageNotFoundError:
        return package, "unknown"


def _compute_node_id(
    lang: str,
    node_kind: str,
    start_byte: int,
    end_byte: int,
    parent_id: str,
) -> str:
    payload = f"{lang}|{node_kind}|{start_byte}|{end_byte}|{parent_id}".encode()
    return hashlib.blake2b(payload, digest_size=8).hexdigest()


def map_tree_sitter_to_uast(
    root: Node,
    language: str,
    map_node_kind: Callable[[Node], str],
    file: str | None = None,
    is_test_node: TestNodeFilter | None = None,
    extract_attributes: Callable[[Node], dict[str, object]] | None = None,
) -> UASTNode:
    """Map a Tree-sitter CST to the normalized UAST representation.

    `is_test_node`, when provided, classifies each node's named siblings in
    one pass to decide which are test-only scaffolding that should be
    excluded from the SIMPLE-relevant AST — see `TestNodeFilter`. Languages
    that don't provide one keep today's behavior of mapping every named
    node.

    `extract_attributes`, when provided, contributes language-specific
    normalized attributes to each mapped UAST node.
    """
    parser_name, parser_version = parser_identity(language)

    def _filtered_named_children(node: Node) -> list[Node]:
        """Named children of `node`, minus any the language's `is_test_node`
        classifier flags as test-only.

        The classifier sees the full named-sibling list in one pass (not a
        single candidate node queried repeatedly) so languages whose test
        markers live on a *separate* sibling — e.g. Rust's `#[cfg(test)]`
        attribute preceding the item it annotates — can correlate adjacent
        siblings in O(n) instead of re-scanning per candidate.
        """
        named = [c for c in node.children if c.is_named]
        if is_test_node is None:
            return named
        dropped = is_test_node(named)
        return [child for child in named if child.id not in dropped]

    # Two-phase iterative traversal — avoids Python recursion limits on deeply
    # nested trees (macro-expanded Rust, minified JS, etc.).
    #
    # Phase 1: pre-order DFS to record visit order and compute stable IDs.
    # Phase 2: reverse pre-order (children before parents) to build UASTNodes.
    # node.id is the tree-sitter C-layer node identity; stable across repeated
    # calls to node.children on the same tree.

    order: list[tuple[Node, str]] = []
    stable_ids: dict[int, str] = {}

    stack: list[tuple[Node, str]] = [(root, "")]
    while stack:
        node, parent_stable_id = stack.pop()

        node_stable_id = _compute_node_id(
            lang=language,
            node_kind=node.type,
            start_byte=node.start_byte,
            end_byte=node.end_byte,
            parent_id=parent_stable_id,
        )
        stable_ids[node.id] = node_stable_id
        order.append((node, node_stable_id))

        for child in reversed(_filtered_named_children(node)):
            stack.append((child, node_stable_id))

    uast_nodes: dict[int, UASTNode] = {}
    for node, node_stable_id in reversed(order):
        named_children = _filtered_named_children(node)
        children = [uast_nodes[c.id] for c in named_children]
        start_point = node.start_point
        end_point = node.end_point
        span = SourceSpan(
            file=file,
            start_byte=node.start_byte,
            end_byte=node.end_byte,
            start_line=start_point[0] + 1,
            start_column=start_point[1],
            end_line=end_point[0] + 1,
            end_column=end_point[1],
        )
        native = NativeRef(
            parser=parser_name,
            parser_version=parser_version,
            node_kind=node.type,
        )
        attributes: dict[str, object] = {"named": node.is_named}
        if extract_attributes is not None:
            attributes.update(extract_attributes(node) or {})
        uast_nodes[node.id] = UASTNode(
            kind=map_node_kind(node),
            lang=language,
            span=span,
            native=native,
            attributes=attributes,
            children=children,
            id=node_stable_id,
        )

    return uast_nodes[root.id]
