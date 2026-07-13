from __future__ import annotations

from tree_sitter import Node

from topos.graphs.uast.mapper_common import map_tree_sitter_to_uast
from topos.graphs.uast.models import UASTNode

_DECLARATION_TYPES = {
    "function_declaration": "FunctionDecl",
    "method_declaration": "MethodDecl",
    "type_declaration": "TypeDecl",
    "const_declaration": "VarDecl",
    "var_declaration": "VarDecl",
    "short_var_declaration": "VarDecl",  # `x := expr`
}

_STATEMENT_TYPES = {
    "if_statement": "IfStmt",
    "for_statement": "ForStmt",  # Go's single loop keyword covers all forms
    "expression_switch_statement": "MatchStmt",
    "type_switch_statement": "MatchStmt",
    "select_statement": "MatchStmt",  # channel-select: structurally a multi-way branch
    "return_statement": "ReturnStmt",
    "break_statement": "BreakStmt",
    "continue_statement": "ContinueStmt",
    "expression_statement": "ExprStmt",
    "assignment_statement": "AssignExpr",
    "inc_statement": "AssignExpr",  # `x++` / `x--`
    # Neither has a dedicated UAST control-flow kind; mapped structurally
    # (by node type, not callee identifier) as plain expression statements,
    # consistent with how `panic(...)` below stays an ordinary CallExpr.
    "go_statement": "ExprStmt",  # `go f()` goroutine launch
    "defer_statement": "ExprStmt",
}

_EXPRESSION_TYPES = {
    "binary_expression": "BinaryExpr",
    "unary_expression": "UnaryExpr",
    "call_expression": "CallExpr",  # covers panic(...) too
    "selector_expression": "MemberExpr",  # `x.y`, `pkg.Func`, method targets
    "index_expression": "MemberExpr",
}

_LITERAL_TYPES = {
    "int_literal",
    "float_literal",
    "imaginary_literal",
    "rune_literal",
    "interpreted_string_literal",
    "raw_string_literal",
    "true",
    "false",
    "nil",
}

_IDENTIFIER_TYPES = {
    "identifier",
    "field_identifier",
    "type_identifier",
    "package_identifier",
}


_TYPE_SPEC_KIND = {
    "interface_type": "interface",
    "struct_type": "struct",
}


def extract_type_attributes(node: Node) -> dict[str, object]:
    """Classify a `type_declaration` as interface/struct via its `type_spec`
    grandchild — the discriminating grammar node lives one level below the
    `TypeDecl`-mapped node itself (Go wraps every type declaration, whether
    interface, struct, or alias, in the same outer `type_declaration`)."""
    if node.type != "type_declaration":
        return {}
    for child in node.named_children:
        if child.type != "type_spec":
            continue
        for grandchild in child.named_children:
            type_kind = _TYPE_SPEC_KIND.get(grandchild.type)
            if type_kind is not None:
                return {"typeKind": type_kind}
    return {}


def map_node_kind(node: Node) -> str:
    if node.type in _DECLARATION_TYPES:
        return _DECLARATION_TYPES[node.type]
    if node.type in _STATEMENT_TYPES:
        return _STATEMENT_TYPES[node.type]
    if node.type in _EXPRESSION_TYPES:
        return _EXPRESSION_TYPES[node.type]
    if node.type in _IDENTIFIER_TYPES:
        return "Identifier"
    if node.type in _LITERAL_TYPES:
        return "Literal"
    if node.type == "source_file":
        return "File"
    return "Unknown"


def map_go_tree_to_uast(root: Node, file: str | None = None) -> UASTNode:
    return map_tree_sitter_to_uast(
        root=root,
        language="go",
        map_node_kind=map_node_kind,
        file=file,
        extract_attributes=extract_type_attributes,
    )
