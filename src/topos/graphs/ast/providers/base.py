from __future__ import annotations

from typing import Protocol

from topos.graphs.ast.types import ParseResult


class AstProvider(Protocol):
    """Protocol for language-aware parser backends."""

    name: str

    def supports(self, language: str) -> bool:
        ...

    def parse(self, source: str, language: str, file: str | None = None) -> ParseResult:
        ...
