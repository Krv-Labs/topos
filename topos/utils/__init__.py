"""
Utils Module
------------
Infrastructure for AST parsing and normalization.

Provides a language-agnostic interface to tree-sitter, designed
for future extension to multiple programming languages.
"""

from topos.utils.tree_sitter import (
    CppParser,
    JavaScriptParser,
    PythonParser,
    RustParser,
    parse_cpp,
    parse_javascript,
    parse_python,
    parse_rust,
)


def parse_source(*args, **kwargs):
    from topos.graphs.ast.dispatch import parse_source as _parse_source

    return _parse_source(*args, **kwargs)


def get_capability_matrix():
    from topos.graphs.ast.dispatch import get_capability_matrix as _matrix

    return _matrix()


__all__ = [
    "parse_source",
    "get_capability_matrix",
    "parse_python",
    "parse_rust",
    "parse_javascript",
    "parse_cpp",
    "PythonParser",
    "RustParser",
    "JavaScriptParser",
    "CppParser",
]
