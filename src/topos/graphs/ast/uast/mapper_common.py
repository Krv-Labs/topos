from __future__ import annotations

from collections.abc import Callable

from tree_sitter import Node

from topos.graphs.ast.uast.models import NativeRef, SourceSpan, UASTNode

PARSER_VERSION = "tree-sitter>=0.23"


def map_tree_sitter_to_uast(
    root: Node,
    language: str,
    map_node_kind: Callable[[Node], str],
    parser_name: str = "tree-sitter",
    parser_version: str = PARSER_VERSION,
    file: str | None = None,
) -> UASTNode:
    def to_uast(node: Node) -> UASTNode:
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
        children = [to_uast(child) for child in node.children if child.is_named]
        return UASTNode(
            kind=map_node_kind(node),
            lang=language,
            span=span,
            native=native,
            attributes={"named": node.is_named},
            children=children,
        )

    return to_uast(root)
