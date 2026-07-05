"""Legacy AST utils — lazy re-exports of dispatch helpers."""

from __future__ import annotations

import importlib
from typing import Any

__all__ = ["AstBackend", "get_capability_matrix", "parse_source"]

_LAZY: dict[str, tuple[str, str]] = {
    "AstBackend": ("topos.graphs.ast.dispatch", "AstBackend"),
    "get_capability_matrix": ("topos.graphs.ast.dispatch", "get_capability_matrix"),
    "parse_source": ("topos.graphs.ast.dispatch", "parse_source"),
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
