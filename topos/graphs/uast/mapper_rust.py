from __future__ import annotations

from tree_sitter import Node

from topos.graphs.uast.mapper_common import map_tree_sitter_to_uast
from topos.graphs.uast.models import UASTNode

_CFG_TEST_MARKER = b"cfg(test)"


def _is_cfg_test_attribute(node: Node) -> bool:
    return (
        node.type == "attribute_item"
        and bool(node.text)
        and _CFG_TEST_MARKER in node.text
    )


def is_test_node(node: Node, siblings: list[Node]) -> bool:
    """Rust's `TestNodePredicate`: drop `#[cfg(test)]`-annotated items.

    Tree-sitter-rust represents an attribute as a *preceding sibling* of the
    item it annotates (both children of the same parent), not as a
    descendant of that item — so this predicate replays the same
    forward scan over `siblings` used before the introduction of the
    language-agnostic filtering hook, evaluated per-candidate: the
    `#[cfg(test)]` attribute itself is dropped, and so is the item
    immediately following it (skipping over any intervening non-`cfg(test)`
    attributes, mirroring the original single-pass behavior exactly).
    """
    pending_test_attr = False
    for sibling in siblings:
        if sibling.type == "attribute_item":
            if _is_cfg_test_attribute(sibling):
                pending_test_attr = True
                if sibling.id == node.id:
                    return True
                continue
            if sibling.id == node.id:
                return False
            continue
        if pending_test_attr:
            pending_test_attr = False
            if sibling.id == node.id:
                return True
            continue
        if sibling.id == node.id:
            return False
    return False


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


def map_rust_tree_to_uast(root: Node, file: str | None = None) -> UASTNode:
    return map_tree_sitter_to_uast(
        root=root,
        language="rust",
        map_node_kind=map_node_kind,
        file=file,
        is_test_node=is_test_node,
    )
