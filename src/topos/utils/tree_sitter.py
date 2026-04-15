"""
Tree-Sitter Module
------------------
Infrastructure for AST parsing and normalization.

This module provides a language-agnostic interface to tree-sitter,
designed for future extension to multiple programming languages.
Currently supports Python, with a clear extension path for others.

Mathematical Context:
    In our categorical framework, parsing acts as a functor from the
    category of source texts to the category of syntax trees. It discards
    surface-level detail (whitespace, comments, formatting) while
    preserving computational structure. This is the left adjoint to
    the 'realization' (pretty-printing) functor that maps trees back to
    text—not a forgetful functor, which goes from more structure to less.

    tree-sitter provides incremental parsing, making it efficient
    for repeated analysis of evolving code—perfect for evaluating
    code as it's being generated or modified.

Usage:
    from topos.utils.tree_sitter import parse_python, PythonParser

    # Quick parsing
    root = parse_python("def foo(): pass")

    # Parser instance for repeated use
    parser = PythonParser()
    root = parser.parse("def bar(): return 42")
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Protocol, runtime_checkable

import tree_sitter_python as ts_python
from tree_sitter import Language, Node, Parser


@runtime_checkable
class LanguageParser(Protocol):
    """Protocol for language-specific parsers."""

    language: str

    def parse(self, source: str) -> Node:
        """Parse source code and return the root AST node."""
        ...


@dataclass
class PythonParser:
    """
    Parser for Python source code using tree-sitter.

    This class wraps tree-sitter's Python parser, providing a
    clean interface for AST generation.

    Attributes:
        language: The language identifier ('python').

    Example:
        parser = PythonParser()
        root = parser.parse("print('hello')")
        for child in root.children:
            print(child.type)
    """

    language: str = "python"

    def __post_init__(self) -> None:
        """Initialize the tree-sitter parser."""
        self._language = Language(ts_python.language())
        self._parser = Parser(self._language)

    def parse(self, source: str) -> Node:
        """
        Parse Python source code into an AST.

        Args:
            source: Python source code as a string.

        Returns:
            The root Node of the parsed AST.

        Raises:
            ValueError: If parsing fails catastrophically.
        """
        source_bytes = source.encode("utf-8")
        tree = self._parser.parse(source_bytes)
        return tree.root_node

    def parse_bytes(self, source: bytes) -> Node:
        """
        Parse Python source code from bytes.

        Args:
            source: Python source code as bytes.

        Returns:
            The root Node of the parsed AST.
        """
        tree = self._parser.parse(source)
        return tree.root_node


_default_python_parser: PythonParser | None = None


def get_python_parser() -> PythonParser:
    """
    Get the shared Python parser instance.

    Returns a singleton parser instance for efficiency when
    parsing multiple files.

    Returns:
        The shared PythonParser instance.
    """
    global _default_python_parser
    if _default_python_parser is None:
        _default_python_parser = PythonParser()
    return _default_python_parser


def parse_python(source: str) -> Node:
    """
    Parse Python source code into an AST.

    Convenience function that uses the shared parser instance.

    Args:
        source: Python source code as a string.

    Returns:
        The root Node of the parsed AST.

    Example:
        root = parse_python("x = 1 + 2")
        assert root.type == "module"
    """
    parser = get_python_parser()
    return parser.parse(source)


def node_text(node: Node, source: str) -> str:
    """
    Extract the source text corresponding to an AST node.

    Args:
        node: The AST node.
        source: The original source code.

    Returns:
        The text slice corresponding to the node.
    """
    return source[node.start_byte : node.end_byte]


def node_to_sexp(node: Node) -> str:
    """
    Convert a node to S-expression format.

    S-expressions are a compact textual representation of tree
    structure, useful for debugging and comparison.

    Args:
        node: The AST node to convert.

    Returns:
        The S-expression string representation.

    Example:
        >>> parse_python("x = 1")
        "(module (expression_statement (assignment ...)))"
    """
    # Some tree-sitter Python bindings expose `sexp()`, others do not.
    sexp_method = getattr(node, "sexp", None)
    if callable(sexp_method):
        return str(sexp_method())

    # Fallback: recursively build a simple S-expression.
    if not node.children:
        return f"({node.type})"
    children = " ".join(node_to_sexp(child) for child in node.children)
    return f"({node.type} {children})"


def find_errors(node: Node) -> list[Node]:
    """
    Find all error nodes in an AST.

    Error nodes indicate syntax errors in the source code.

    Args:
        node: The root node to search from.

    Returns:
        A list of all ERROR nodes in the tree.
    """
    errors: list[Node] = []

    def walk(n: Node) -> None:
        if n.type == "ERROR" or n.is_missing:
            errors.append(n)
        for child in n.children:
            walk(child)

    walk(node)
    return errors
