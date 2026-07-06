"""
AST Representation Sub-package
------------------------------
Wraps the tree-sitter AST as a :class:`Representation`.
"""

from __future__ import annotations

import importlib
from typing import Any

__all__ = [
    "ASTRepresentation",
    "AstBackend",
    "parse_source",
    "get_capability_matrix",
]

_LAZY: dict[str, tuple[str, str]] = {
    "ASTRepresentation": ("topos.graphs.ast.object", "ASTRepresentation"),
    "AstBackend": ("topos.graphs.ast.dispatch", "AstBackend"),
    "parse_source": ("topos.graphs.ast.dispatch", "parse_source"),
    "get_capability_matrix": ("topos.graphs.ast.dispatch", "get_capability_matrix"),
}


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
