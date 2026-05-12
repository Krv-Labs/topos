from __future__ import annotations

from tree_sitter import Node

from topos.graphs.ast.uast.mapper_common import map_tree_sitter_to_uast
from topos.graphs.ast.uast.models import UASTNode


def map_python_tree_to_uast(root: Node, file: str | None = None) -> UASTNode:
    return map_tree_sitter_to_uast(root=root, language="python", file=file)
