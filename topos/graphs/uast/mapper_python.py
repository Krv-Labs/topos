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


def is_test_node(siblings: list[Node]) -> set[int]:
    """Python's `TestNodeFilter`: drop `if __name__ == "__main__":` guards.

    The guard is fully self-contained (condition + body live under the
    `if_statement` node itself), so unlike Rust's `#[cfg(test)]` this needs
    no cross-sibling correlation — each candidate is classified purely from
    its own subtree, which takes the guard's body with it once dropped.
    """

    def _is_guard(node: Node) -> bool:
        if node.type != "if_statement":
            return False
        condition = node.child_by_field_name("condition")
        if condition is None or not _is_name_equals_main(condition):
            return False
        # Only a bare guard is pure entrypoint scaffolding. A guard carrying
        # an `else`/`elif` holds real fallback logic; dropping the whole
        # `if_statement` would silently discard that branch, so keep it.
        # (A full fix — drop only the `__main__` consequence while retaining
        # the alternative — needs subtree rewriting, which this drop-by-id
        # filter interface intentionally doesn't support.)
        return node.child_by_field_name("alternative") is None

    return {sibling.id for sibling in siblings if _is_guard(sibling)}


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


# First-pass, name-based Abstractness heuristic — no import-alias resolution,
# so `from foo import ABC as Base` won't be recognized. Good enough for the
# overwhelmingly common `abc.ABC` / `typing.Protocol` spellings.
_ABSTRACT_BASE_MARKERS = (b"ABC", b"Protocol", b"ABCMeta")


def _has_abstract_base(node: Node) -> bool:
    superclasses = node.child_by_field_name("superclasses")
    if superclasses is None:
        return False
    text = superclasses.text or b""
    return any(marker in text for marker in _ABSTRACT_BASE_MARKERS)


def _has_abstractmethod(node: Node) -> bool:
    body = node.child_by_field_name("body")
    if body is None:
        return False
    for child in body.named_children:
        if child.type != "decorated_definition":
            continue
        for grandchild in child.named_children:
            if grandchild.type == "decorator" and b"abstractmethod" in (
                grandchild.text or b""
            ):
                return True
    return False


def extract_type_attributes(node: Node) -> dict[str, object]:
    if node.type != "class_definition":
        return {}
    is_abstract = _has_abstract_base(node) or _has_abstractmethod(node)
    return {"typeKind": "abstractClass" if is_abstract else "class"}


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
        extract_attributes=extract_type_attributes,
    )
