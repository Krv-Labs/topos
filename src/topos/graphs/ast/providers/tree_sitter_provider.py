from __future__ import annotations

from dataclasses import dataclass

from topos.graphs.ast.types import ParseResult, ParserProvenance
from topos.graphs.ast.uast.mapper_cpp import map_cpp_tree_to_uast
from topos.graphs.ast.uast.mapper_javascript import map_javascript_tree_to_uast
from topos.graphs.ast.uast.mapper_python import map_python_tree_to_uast
from topos.graphs.ast.uast.mapper_rust import map_rust_tree_to_uast
from topos.utils.tree_sitter import (
    parse_cpp,
    parse_javascript,
    parse_python,
    parse_rust,
)

_TREE_SITTER_PARSE = {
    "python": parse_python,
    "rust": parse_rust,
    "javascript": parse_javascript,
    "cpp": parse_cpp,
}

_TREE_SITTER_UAST = {
    "python": map_python_tree_to_uast,
    "rust": map_rust_tree_to_uast,
    "javascript": map_javascript_tree_to_uast,
    "cpp": map_cpp_tree_to_uast,
}


@dataclass
class TreeSitterProvider:
    name: str = "tree-sitter"
    parser_version: str = "tree-sitter>=0.23"

    def supports(self, language: str) -> bool:
        return language in _TREE_SITTER_PARSE

    def parse(self, source: str, language: str, file: str | None = None) -> ParseResult:
        if language not in _TREE_SITTER_PARSE:
            raise ValueError(f"Language '{language}' is not supported by tree-sitter")

        root = _TREE_SITTER_PARSE[language](source)
        uast_root = _TREE_SITTER_UAST[language](root, file=file)
        provenance = ParserProvenance(
            parser=self.name,
            parser_version=self.parser_version,
            node_kind=root.type,
        )
        return ParseResult(
            root=root,
            source=source,
            language=language,
            provenance=provenance,
            native_ast=None,
            uast_root=uast_root,
        )
