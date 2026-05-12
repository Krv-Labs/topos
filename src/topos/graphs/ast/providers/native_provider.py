from __future__ import annotations

import ast as py_ast
from dataclasses import dataclass
from typing import Any

from topos.graphs.ast.providers.tree_sitter_provider import TreeSitterProvider
from topos.graphs.ast.types import ParseResult, ParserProvenance


@dataclass
class NativeAstProvider:
    """
    Industry-standard native parser provider with graceful fallback behavior.

    Python is supported via stdlib `ast`; other languages currently surface
    explicit unsupported capability and can be added without changing callers.
    """

    name: str = "native"
    parser_version: str = "python-stdlib-ast"

    def __post_init__(self) -> None:
        self._fallback_provider = TreeSitterProvider()

    def supports(self, language: str) -> bool:
        return language == "python"

    def parse(self, source: str, language: str, file: str | None = None) -> ParseResult:
        # Always build a tree-sitter root so existing ProgramObject metrics continue
        # to operate over tree traversal while native/UAST layers mature.
        fallback = self._fallback_provider.parse(source, language=language, file=file)

        native_ast: Any | None = None
        parser_name = "tree-sitter"
        parser_version = fallback.provenance.parser_version

        if language == "python":
            try:
                native_ast = py_ast.parse(source)
                parser_name = "cpython-ast"
                parser_version = self.parser_version
            except SyntaxError:
                # Keep working artifact path even on syntax errors.
                native_ast = None

        provenance = ParserProvenance(
            parser=parser_name,
            parser_version=parser_version,
            node_kind=fallback.root.type,
        )
        return ParseResult(
            root=fallback.root,
            source=source,
            language=language,
            provenance=provenance,
            native_ast=native_ast,
            uast_root=fallback.uast_root,
        )
