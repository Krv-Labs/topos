from __future__ import annotations

from tree_sitter import Node

from topos.graphs.uast.mapper_common import map_tree_sitter_to_uast
from topos.graphs.uast.models import UASTNode

_DECLARATION_TYPES = {
    "function_definition": "FunctionDecl",
    "class_definition": "TypeDecl",
    "struct_item": "TypeDecl",
    "enum_item": "TypeDecl",
    "impl_item": "TypeDecl",
    "trait_item": "TypeDecl",
    "function_item": "FunctionDecl",
    "method_definition": "MethodDecl",
    "lexical_declaration": "VarDecl",
    "variable_declaration": "VarDecl",
}

# Martin Abstractness classification for TypeDecl nodes. `impl_item` is
# intentionally absent: it implements an existing type rather than declaring
# a new one, so it must not be double-counted in the abstract/concrete ratio.
_TYPE_KIND = {
    "trait_item": "trait",
    "struct_item": "struct",
    "enum_item": "enum",
}

_STATEMENT_TYPES = {
    # Rust grammar exposes the control-flow primitives as expressions,
    # not statements (e.g. `if_expression`).  Both spellings map here so
    # the CFG builder picks them up uniformly across languages.
    "if_statement": "IfStmt",
    "if_expression": "IfStmt",
    "for_statement": "ForStmt",
    "for_expression": "ForStmt",
    "while_statement": "WhileStmt",
    "while_expression": "WhileStmt",
    "loop_expression": "WhileStmt",  # Rust `loop { ... }`
    "match_statement": "MatchStmt",
    "match_expression": "MatchStmt",
    "return_statement": "ReturnStmt",
    "return_expression": "ReturnStmt",
    "break_statement": "BreakStmt",
    "break_expression": "BreakStmt",
    "continue_statement": "ContinueStmt",
    "continue_expression": "ContinueStmt",
    "throw_statement": "ThrowStmt",
    "try_statement": "TryStmt",
    "expression_statement": "ExprStmt",
    "let_declaration": "VarDecl",
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


def extract_type_attributes(node: Node) -> dict[str, object]:
    type_kind = _TYPE_KIND.get(node.type)
    return {"typeKind": type_kind} if type_kind is not None else {}


def map_rust_tree_to_uast(root: Node, file: str | None = None) -> UASTNode:
    return map_tree_sitter_to_uast(
        root=root,
        language="rust",
        map_node_kind=map_node_kind,
        file=file,
        extract_attributes=extract_type_attributes,
    )
