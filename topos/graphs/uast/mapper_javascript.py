from __future__ import annotations

from tree_sitter import Node

from topos.graphs.uast.mapper_common import map_tree_sitter_to_uast
from topos.graphs.uast.models import UASTNode

_DECLARATION_TYPES = {
    "function_definition": "FunctionDecl",
    "function_declaration": "FunctionDecl",  # JS native name for top-level fn
    "function": "FunctionDecl",
    "function_expression": "FunctionDecl",
    "arrow_function": "FunctionDecl",
    "class_definition": "TypeDecl",
    "class_declaration": "TypeDecl",
    "struct_item": "TypeDecl",
    "enum_item": "TypeDecl",
    "impl_item": "TypeDecl",
    "function_item": "FunctionDecl",
    "method_definition": "MethodDecl",
    "lexical_declaration": "VarDecl",
    "variable_declaration": "VarDecl",
}

_STATEMENT_TYPES = {
    "if_statement": "IfStmt",
    "for_statement": "ForStmt",
    "while_statement": "WhileStmt",
    "match_statement": "MatchStmt",
    "return_statement": "ReturnStmt",
    "break_statement": "BreakStmt",
    "continue_statement": "ContinueStmt",
    "throw_statement": "ThrowStmt",
    "try_statement": "TryStmt",
    "expression_statement": "ExprStmt",
}

_EXPRESSION_TYPES = {
    "assignment": "AssignExpr",
    "augmented_assignment": "AssignExpr",
    "binary_expression": "BinaryExpr",
    "boolean_operator": "BinaryExpr",
    "unary_expression": "UnaryExpr",
    "call_expression": "CallExpr",
    "member_expression": "MemberExpr",
    "field_expression": "MemberExpr",
    "subscript": "MemberExpr",
}


def map_node_kind(node: Node) -> str:
    if node.type in _DECLARATION_TYPES:
        return _DECLARATION_TYPES[node.type]
    if node.type in _STATEMENT_TYPES:
        return _STATEMENT_TYPES[node.type]
    if node.type in _EXPRESSION_TYPES:
        return _EXPRESSION_TYPES[node.type]
    if node.type == "identifier":
        return "Identifier"
    if node.type.endswith("literal") or node.type in {"string", "integer", "float"}:
        return "Literal"
    if node.type in {"module", "program", "translation_unit", "source_file"}:
        return "File"
    return "Unknown"


def map_javascript_tree_to_uast(root: Node, file: str | None = None) -> UASTNode:
    # Deliberately no `extract_attributes`/`typeKind` hook here, unlike the
    # Rust/Python/Go/TypeScript mappers: plain JS has no `interface` or
    # `abstract class` syntax at all (that's a TypeScript-only extension of
    # this shared grammar — see mapper_typescript.py), so there's nothing
    # in a .js file's grammar to ever classify as abstract vs. concrete.
    # Martin's Abstractness metric (mdg.abstractness, issue #124) is
    # therefore not meaningful for JavaScript; see the language allowlist
    # in topos/graphs/uast/object.py.
    return map_tree_sitter_to_uast(
        root=root,
        language="javascript",
        map_node_kind=map_node_kind,
        file=file,
    )
