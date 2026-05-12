from __future__ import annotations

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
"""

import hashlib
import sys
from collections.abc import Callable
from importlib.metadata import PackageNotFoundError, version

from tree_sitter import Node

from topos.graphs.ast.uast.models import NativeRef, SourceSpan, UASTNode

_TREE_SITTER_PACKAGE = {
    "python": "tree-sitter-python",
    "rust": "tree-sitter-rust",
    "javascript": "tree-sitter-javascript",
    "cpp": "tree-sitter-cpp",
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
) -> UASTNode:
    parser_name, parser_version = parser_identity(language)

    def to_uast(node: Node, parent_id: str = "") -> UASTNode:
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
        node_id = _compute_node_id(
            lang=language,
            node_kind=native.node_kind,
            start_byte=span.start_byte,
            end_byte=span.end_byte,
            parent_id=parent_id,
        )
        children = [
            to_uast(child, parent_id=node_id)
            for child in node.children
            if child.is_named
        ]
        return UASTNode(
            kind=map_node_kind(node),
            lang=language,
            span=span,
            native=native,
            attributes={"named": node.is_named},
            children=children,
            id=node_id,
        )

    return to_uast(root)
