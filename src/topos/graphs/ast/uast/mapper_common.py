from __future__ import annotations

from tree_sitter import Node

from topos.graphs.ast.uast.models import NativeRef, SourceSpan, UASTNode

PARSER_VERSION = "tree-sitter>=0.23"

_DECLARATION_TYPES = {
    "function_definition": "FunctionDecl",
    "class_definition": "TypeDecl",
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


def map_tree_sitter_to_uast(
    root: Node,
    language: str,
    parser_name: str = "tree-sitter",
    parser_version: str = PARSER_VERSION,
    file: str | None = None,
) -> UASTNode:
    def to_uast(node: Node) -> UASTNode:
        start_point = node.start_point
        end_point = node.end_point
        span = SourceSpan(
            file=file,
            start_byte=node.start_byte,
            end_byte=node.end_byte,
            start_line=start_point[0] + 1,
            start_column=start_point[1],
            end_line=end_point[0] + 1,
            end_column=end_point[1],
        )
        native = NativeRef(
            parser=parser_name,
            parser_version=parser_version,
            node_kind=node.type,
        )
        children = [to_uast(child) for child in node.children if child.is_named]
        return UASTNode(
            kind=map_node_kind(node),
            lang=language,
            span=span,
            native=native,
            attributes={"named": node.is_named},
            children=children,
        )

    return to_uast(root)
