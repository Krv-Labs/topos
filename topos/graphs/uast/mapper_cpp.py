from __future__ import annotations

from tree_sitter import Node

from topos.graphs.uast.mapper_common import map_tree_sitter_to_uast
from topos.graphs.uast.models import UASTNode

# Real tree-sitter-cpp grammar node names (verified against the vendored
# grammar directly — the previous dict here was copy-pasted from the
# Python/Rust mappers and used node names tree-sitter-cpp doesn't emit at
# all, e.g. `class_definition`/`struct_item`, so every C++ type
# declaration silently fell to `Unknown`; see issue #158).
_DECLARATION_TYPES = {
    "function_definition": "FunctionDecl",  # free functions AND in-class
    # method *definitions* (with a body) — tree-sitter-cpp doesn't
    # distinguish the two at this node.
    "class_specifier": "TypeDecl",
    "struct_specifier": "TypeDecl",
    "enum_specifier": "TypeDecl",
    "union_specifier": "TypeDecl",
}

# `declaration` (free-standing) and `field_declaration` (inside a class
# body) are dual-purpose in the grammar: a variable/member declaration
# (`int x;`) and a declaration-only function/method signature with no
# body (`void f(int);`, or a pure-virtual `virtual double area() const =
# 0;`) both use the same wrapper node, distinguished only by whether a
# `function_declarator` child is present.
_DECLARATION_ONLY_KINDS = {
    "declaration": "VarDecl",
    "field_declaration": "VarDecl",
}


def _has_function_declarator(node: Node) -> bool:
    return any(child.type == "function_declarator" for child in node.children)


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
    "assignment_expression": "AssignExpr",
    "binary_expression": "BinaryExpr",
    "unary_expression": "UnaryExpr",
    "call_expression": "CallExpr",
    "field_expression": "MemberExpr",
    "subscript_expression": "MemberExpr",
}


def map_node_kind(node: Node) -> str:
    if node.type in _DECLARATION_TYPES:
        return _DECLARATION_TYPES[node.type]
    if node.type in _DECLARATION_ONLY_KINDS:
        return "FunctionDecl" if _has_function_declarator(node) else "VarDecl"
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


# Martin Abstractness classification (issue #124/#158): a class/struct is
# abstract iff it declares at least one pure-virtual method
# (`virtual ... = 0;`) — the C++ idiom for an interface/abstract base
# class. `enum`/`union` are always concrete.
_TYPE_KIND = {
    "class_specifier": "class",
    "struct_specifier": "struct",
    "enum_specifier": "enum",
    "union_specifier": "union",
}


def _is_pure_virtual(field_decl: Node) -> bool:
    """True for a declaration-only method signature with a `= 0`
    pure-specifier, e.g. `virtual double area() const = 0;`."""
    if not _has_function_declarator(field_decl):
        return False
    return any(
        child.type == "number_literal" and child.text == b"0"
        for child in field_decl.named_children
    )


def _has_pure_virtual_method(type_node: Node) -> bool:
    for child in type_node.children:
        if child.type != "field_declaration_list":
            continue
        return any(
            member.type == "field_declaration" and _is_pure_virtual(member)
            for member in child.named_children
        )
    return False


def extract_type_attributes(node: Node) -> dict[str, object]:
    type_kind = _TYPE_KIND.get(node.type)
    if type_kind is None:
        return {}
    is_class_like = node.type in {"class_specifier", "struct_specifier"}
    if is_class_like and _has_pure_virtual_method(node):
        return {"typeKind": "abstractClass"}
    return {"typeKind": type_kind}


def map_cpp_tree_to_uast(root: Node, file: str | None = None) -> UASTNode:
    return map_tree_sitter_to_uast(
        root=root,
        language="cpp",
        map_node_kind=map_node_kind,
        file=file,
        extract_attributes=extract_type_attributes,
    )
