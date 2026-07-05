"""
Utils Module
------------
Infrastructure for AST parsing and normalization.

Provides a language-agnostic interface to tree-sitter, designed
for future extension to multiple programming languages.
"""

from __future__ import annotations

import importlib
from typing import Any

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

_LAZY: dict[str, tuple[str, str]] = {
    "parse_python": ("topos.utils.tree_sitter", "parse_python"),
    "parse_rust": ("topos.utils.tree_sitter", "parse_rust"),
    "parse_javascript": ("topos.utils.tree_sitter", "parse_javascript"),
    "parse_cpp": ("topos.utils.tree_sitter", "parse_cpp"),
    "PythonParser": ("topos.utils.tree_sitter", "PythonParser"),
    "RustParser": ("topos.utils.tree_sitter", "RustParser"),
    "JavaScriptParser": ("topos.utils.tree_sitter", "JavaScriptParser"),
    "CppParser": ("topos.utils.tree_sitter", "CppParser"),
}


def parse_source(*args: Any, **kwargs: Any) -> Any:
    from topos.graphs.ast.dispatch import parse_source as _parse_source

    return _parse_source(*args, **kwargs)


def get_capability_matrix() -> Any:
    from topos.graphs.ast.dispatch import get_capability_matrix as _matrix

    return _matrix()


def __getattr__(name: str) -> Any:
    if name not in _LAZY:
        raise AttributeError(f"module {__name__!r} has no attribute {name!r}")
    module_name, attr_name = _LAZY[name]
    module = importlib.import_module(module_name)
    value = getattr(module, attr_name)
    globals()[name] = value
    return value


def __dir__() -> list[str]:
    return sorted(__all__)
