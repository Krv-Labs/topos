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

import tree_sitter_cpp as ts_cpp
import tree_sitter_javascript as ts_javascript
import tree_sitter_python as ts_python
import tree_sitter_rust as ts_rust
import tree_sitter_typescript as ts_typescript
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


@dataclass
class RustParser:
    """Parser for Rust source code using tree-sitter."""

    language: str = "rust"

    def __post_init__(self) -> None:
        self._language = Language(ts_rust.language())
        self._parser = Parser(self._language)

    def parse(self, source: str) -> Node:
        source_bytes = source.encode("utf-8")
        tree = self._parser.parse(source_bytes)
        return tree.root_node


@dataclass
class JavaScriptParser:
    """Parser for JavaScript source code using tree-sitter."""

    language: str = "javascript"

    def __post_init__(self) -> None:
        self._language = Language(ts_javascript.language())
        self._parser = Parser(self._language)

    def parse(self, source: str) -> Node:
        source_bytes = source.encode("utf-8")
        tree = self._parser.parse(source_bytes)
        return tree.root_node


@dataclass
class TypeScriptParser:
    """Parser for TypeScript / TSX using tree-sitter-typescript / tree-sitter-tsx."""

    language: str = "typescript"
    _is_tsx: bool = False

    def __post_init__(self) -> None:
        lang_fn = (
            ts_typescript.language_tsx
            if self._is_tsx
            else ts_typescript.language_typescript
        )
        self._language = Language(lang_fn())
        self._parser = Parser(self._language)

    def parse(self, source: str) -> Node:
        source_bytes = source.encode("utf-8")
        tree = self._parser.parse(source_bytes)
        return tree.root_node


@dataclass
class CppParser:
    """Parser for C++ source code using tree-sitter."""

    language: str = "cpp"

    def __post_init__(self) -> None:
        self._language = Language(ts_cpp.language())
        self._parser = Parser(self._language)

    def parse(self, source: str) -> Node:
        source_bytes = source.encode("utf-8")
        tree = self._parser.parse(source_bytes)
        return tree.root_node


_default_python_parser: PythonParser | None = None
_default_rust_parser: RustParser | None = None
_default_javascript_parser: JavaScriptParser | None = None
_default_cpp_parser: CppParser | None = None
_default_typescript_parser: TypeScriptParser | None = None
_default_tsx_parser: TypeScriptParser | None = None


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


def get_rust_parser() -> RustParser:
    """Get the shared Rust parser instance."""
    global _default_rust_parser
    if _default_rust_parser is None:
        _default_rust_parser = RustParser()
    return _default_rust_parser


def get_javascript_parser() -> JavaScriptParser:
    """Get the shared JavaScript parser instance."""
    global _default_javascript_parser
    if _default_javascript_parser is None:
        _default_javascript_parser = JavaScriptParser()
    return _default_javascript_parser


def get_tsx_parser() -> TypeScriptParser:
    """Shared parser for ``.tsx`` (JSX) sources."""
    global _default_tsx_parser
    if _default_tsx_parser is None:
        _default_tsx_parser = TypeScriptParser(language="typescript", _is_tsx=True)
    return _default_tsx_parser


def get_typescript_parser() -> TypeScriptParser:
    """Shared parser for ``.ts`` sources (non-TSX grammar)."""
    global _default_typescript_parser
    if _default_typescript_parser is None:
        _default_typescript_parser = TypeScriptParser(
            language="typescript", _is_tsx=False
        )
    return _default_typescript_parser


def get_cpp_parser() -> CppParser:
    """Get the shared C++ parser instance."""
    global _default_cpp_parser
    if _default_cpp_parser is None:
        _default_cpp_parser = CppParser()
    return _default_cpp_parser


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


def parse_rust(source: str) -> Node:
    """Parse Rust source code into an AST."""
    return get_rust_parser().parse(source)


def parse_javascript(source: str) -> Node:
    """Parse JavaScript source code into an AST."""
    return get_javascript_parser().parse(source)


def parse_typescript(source: str, file: str | None = None) -> Node:
    """Parse TypeScript or TSX; uses the TSX grammar when *file* ends with ``.tsx``."""
    parser = (
        get_tsx_parser()
        if file and str(file).endswith(".tsx")
        else get_typescript_parser()
    )
    return parser.parse(source)


def parse_cpp(source: str) -> Node:
    """Parse C++ source code into an AST."""
    return get_cpp_parser().parse(source)


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
