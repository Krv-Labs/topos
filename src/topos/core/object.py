"""
Object Module
-------------
In Category Theory, an 'Object' is an abstract entity that serves as
the domain or codomain of morphisms. In the category of Programs,
we model the Abstract Syntax Tree (AST) as our primary object.

Mathematical Inspiration:
    An object in our category represents the 'shape' or 'structure' of
    a computation—not what the program does, but how it is organized.
    Two programs with isomorphic ASTs are considered structurally equivalent,
    even if their surface syntax differs.

    This abstraction allows us to reason about code structurally:
    transformations (refactorings) that preserve the AST structure
    are isomorphisms in the category of Programs.
"""

from __future__ import annotations

from collections.abc import Iterator
from dataclasses import dataclass, field
from typing import Any
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from tree_sitter import Node


@dataclass
class ProgramObject:
    """
    The AST lifted into the category of Programs.

    A ProgramObject wraps a tree-sitter AST node and provides methods
    for structural analysis. It represents the 'shape' of code—the
    invariant structure that remains after stripping away surface syntax.

    Attributes:
        root: The root node of the parsed AST.
        source: The original source code (for reference).
        language: The programming language of the source.

    Categorical Interpretation:
        Objects in our category are ASTs. A morphism f: A → B represents
        a program that transforms computations of shape A into shape B.
    """

    root: Node
    source: str
    language: str = "python"
    native_ast: Any | None = field(default=None, repr=False)
    uast_root: Any | None = field(default=None, repr=False)
    parser_name: str = "tree-sitter"
    parser_version: str = "tree-sitter>=0.23"
    native_node_kind: str = "module"
    _node_count: int | None = field(default=None, repr=False)

    @property
    def node_count(self) -> int:
        """Total number of nodes in the AST."""
        if self._node_count is None:
            self._node_count = self._count_nodes(self.root)
        return self._node_count

    @property
    def depth(self) -> int:
        """Maximum depth of the AST."""
        return self._calculate_depth(self.root)

    @property
    def is_valid(self) -> bool:
        """Check if the AST has no syntax errors."""
        return not self.root.has_error

    def traverse(self) -> Iterator[Node]:
        """
        Depth-first traversal of all nodes.

        Yields:
            Each node in the AST in depth-first order.
        """
        yield from self._traverse_node(self.root)

    def nodes_of_type(self, *types: str) -> Iterator[Node]:
        """
        Find all nodes matching the given type(s).

        Args:
            types: Node type strings to match (e.g., 'function_definition').

        Yields:
            Nodes whose type matches any of the given types.
        """
        for node in self.traverse():
            if node.type in types:
                yield node

    def _traverse_node(self, node: Node) -> Iterator[Node]:
        """Recursive depth-first traversal helper."""
        yield node
        for child in node.children:
            yield from self._traverse_node(child)

    def _count_nodes(self, node: Node) -> int:
        """Count total nodes in subtree."""
        return 1 + sum(self._count_nodes(child) for child in node.children)

    def _calculate_depth(self, node: Node, current: int = 0) -> int:
        """Calculate maximum depth of subtree."""
        if not node.children:
            return current
        return max(self._calculate_depth(child, current + 1) for child in node.children)

    def __hash__(self) -> int:
        """Hash based on source content."""
        return hash(self.source)

    def __eq__(self, other: object) -> bool:
        """Structural equality based on AST."""
        if not isinstance(other, ProgramObject):
            return NotImplemented
        return self.source == other.source and self.language == other.language
