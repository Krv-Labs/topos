from __future__ import annotations

from tree_sitter import Node

from topos.graphs.uast.mapper_common import map_tree_sitter_to_uast
from topos.graphs.uast.models import UASTNode

_DUNDER_NAME = b"__name__"
_DUNDER_MAIN = b"__main__"


def _is_name_equals_main(condition: Node) -> bool:
    """True if `condition` is `__name__ == "__main__"` (in either order).

    Tree-sitter-python's `comparison_operator` node stores its operator(s)
    under the `operators` field and leaves the operand nodes unlabeled, so
    operands are everything that *isn't* the `operators` field.
    """
    if condition.type != "comparison_operator":
        return False

    operators: list[str] = []
    operands: list[Node] = []
    for index, child in enumerate(condition.children):
        if condition.field_name_for_child(index) == "operators":
            operators.append(child.type)
        else:
            operands.append(child)

    if operators != ["=="] or len(operands) != 2:
        return False

    stripped = {(operand.text or b"").strip(b"'\"") for operand in operands}
    return stripped == {_DUNDER_NAME, _DUNDER_MAIN}


def is_test_node(node: Node, siblings: list[Node]) -> bool:
    """Python's `TestNodePredicate`: drop `if __name__ == "__main__":` guards.

    The guard is fully self-contained (condition + body live under the
    `if_statement` node itself), so unlike Rust's `#[cfg(test)]` this needs
    no sibling correlation — the whole subtree is dropped once the
    condition matches, which takes the guard's body with it.
    """
    del siblings  # unused: no sibling context needed for this predicate
    if node.type != "if_statement":
        return False
    condition = node.child_by_field_name("condition")
    return condition is not None and _is_name_equals_main(condition)


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
    "call": "CallExpr",  # Python tree-sitter grammar names it `call`
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


def map_python_tree_to_uast(root: Node, file: str | None = None) -> UASTNode:
    return map_tree_sitter_to_uast(
        root=root,
        language="python",
        map_node_kind=map_node_kind,
        file=file,
        is_test_node=is_test_node,
    )
