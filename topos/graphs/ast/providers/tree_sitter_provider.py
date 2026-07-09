from __future__ import annotations

from dataclasses import dataclass

from topos.graphs.ast.types import ParseResult, ParserProvenance
from topos.graphs.uast.mapper_common import parser_identity
from topos.graphs.uast.mapper_cpp import map_cpp_tree_to_uast
from topos.graphs.uast.mapper_go import map_go_tree_to_uast
from topos.graphs.uast.mapper_javascript import map_javascript_tree_to_uast
from topos.graphs.uast.mapper_python import map_python_tree_to_uast
from topos.graphs.uast.mapper_rust import map_rust_tree_to_uast
from topos.graphs.uast.mapper_typescript import map_typescript_tree_to_uast
from topos.utils.tree_sitter import (
    parse_cpp,
    parse_go,
    parse_javascript,
    parse_python,
    parse_rust,
    parse_typescript,
)

_TREE_SITTER_PARSE = {
    "python": parse_python,
    "rust": parse_rust,
    "javascript": parse_javascript,
    "typescript": parse_typescript,
    "cpp": parse_cpp,
    "go": parse_go,
}

_TREE_SITTER_UAST = {
    "python": map_python_tree_to_uast,
    "rust": map_rust_tree_to_uast,
    "javascript": map_javascript_tree_to_uast,
    "typescript": map_typescript_tree_to_uast,
    "cpp": map_cpp_tree_to_uast,
    "go": map_go_tree_to_uast,
}


@dataclass
class TreeSitterProvider:
    name: str = "tree-sitter"

    def supports(self, language: str) -> bool:
        return language in _TREE_SITTER_PARSE

    def parse(self, source: str, language: str, file: str | None = None) -> ParseResult:
        if language not in _TREE_SITTER_PARSE:
            raise ValueError(f"Language '{language}' is not supported by tree-sitter")

        if language == "typescript":
            root = parse_typescript(source, file=file)
        else:
            root = _TREE_SITTER_PARSE[language](source)
        uast_root = _TREE_SITTER_UAST[language](root, file=file)
        parser_name, parser_version = parser_identity(language)
        provenance = ParserProvenance(
            parser=parser_name,
            parser_version=parser_version,
            node_kind=root.type,
        )
        return ParseResult(
            root=root,
            source=source,
            language=language,
            provenance=provenance,
            native_ast=None,
            uast_root=uast_root,
            has_errors=root.has_error,
        )
