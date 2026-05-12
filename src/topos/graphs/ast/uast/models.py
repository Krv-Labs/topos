from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any


@dataclass(frozen=True)
class SourceSpan:
    file: str | None
    start_byte: int
    end_byte: int
    start_line: int
    start_column: int
    end_line: int
    end_column: int


@dataclass(frozen=True)
class NativeRef:
    parser: str
    parser_version: str
    node_kind: str


@dataclass
class UASTNode:
    """
    Language-normalized node carrying provenance and source spans.

    The `kind` values intentionally follow the industry-standard reference
    in docs/uast-industry-standards.md.
    """

    kind: str
    lang: str
    span: SourceSpan
    native: NativeRef
    attributes: dict[str, Any] = field(default_factory=dict)
    children: list["UASTNode"] = field(default_factory=list)
