from __future__ import annotations

from dataclasses import dataclass
from typing import Any

from tree_sitter import Node


@dataclass(frozen=True)
class ParserProvenance:
    """Metadata describing how AST artifacts were produced."""

    parser: str
    parser_version: str
    node_kind: str


@dataclass(frozen=True)
class ParseResult:
    """Container for language parsing artifacts used by topos."""

    root: Node
    source: str
    language: str
    provenance: ParserProvenance
    native_ast: Any | None = None
    uast_root: Any | None = None
    has_errors: bool = False
