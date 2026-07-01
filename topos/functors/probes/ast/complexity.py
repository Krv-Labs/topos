"""
Per-Function Complexity Analysis
--------------------------------

Provides a function-level breakdown of complexity using the AST.
This is separate from the CFG-based module-level cyclomatic complexity used
in the main program evaluation (SIMPLE generator).
"""

from __future__ import annotations

from dataclasses import dataclass

from topos.core.object import ProgramObject

_FUNCTION_NODE_TYPES = ("function_definition", "async_function_definition")
_SCOPE_NODE_TYPES = (*_FUNCTION_NODE_TYPES, "class_definition")

DECISION_NODE_TYPES = frozenset(
    {
        "if_statement",
        "elif_clause",
        "for_statement",
        "while_statement",
        "except_clause",
        "with_statement",
        "assert_statement",
        "conditional_expression",  # ternary
        "boolean_operator",  # and/or short-circuit
        "match_statement",  # Python 3.10+
        "case_clause",
        "list_comprehension",
        "dictionary_comprehension",
        "set_comprehension",
        "generator_expression",
    }
)

DECISION_UAST_KINDS = frozenset(
    {
        "IfStmt",
        "ForStmt",
        "WhileStmt",
        "MatchStmt",
        "TryStmt",
    }
)


def _calculate_node_complexity(ast: ProgramObject) -> int:
    """Internal helper to calculate complexity of a specific node/sub-tree."""
    if ast.uast_root is not None:
        return _calculate_cyclomatic_complexity_uast(ast.uast_root)

    decision_count = 0

    for node in ast.traverse():
        if node.type in DECISION_NODE_TYPES:
            decision_count += 1
            if node.type == "boolean_operator":
                decision_count += _count_boolean_operators(node) - 1

    return decision_count + 1


def _calculate_cyclomatic_complexity_uast(root) -> int:
    decision_count = 0

    def walk(node) -> None:
        nonlocal decision_count
        kind = getattr(node, "kind", "")
        if kind in DECISION_UAST_KINDS:
            decision_count += 1
        if kind == "BinaryExpr":
            operator = getattr(node, "attributes", {}).get("operator")
            if operator in {"and", "or", "&&", "||"}:
                decision_count += 1
        for child in getattr(node, "children", []):
            walk(child)

    walk(root)
    return decision_count + 1


def _count_boolean_operators(node) -> int:
    """Count chained boolean operators (and/or)."""
    count = 1
    for child in node.children:
        if child.type == "boolean_operator":
            count += _count_boolean_operators(child)
    return count


def _extract_function_name(node, source_bytes: bytes) -> str | None:
    for child in node.children:
        if child.type == "identifier":
            return source_bytes[child.start_byte : child.end_byte].decode("utf-8")
    return None


def calculate_function_complexities(ast: ProgramObject) -> dict[str, int]:
    """
    Calculate cyclomatic complexity for each function in the AST.

    Args:
        ast: The ProgramObject to analyze.

    Returns:
        A dictionary mapping function names to their complexity scores.
    """
    complexities: dict[str, int] = {}
    source_bytes = ast.source.encode("utf-8")

    for node in ast.nodes_of_type("function_definition", "async_function_definition"):
        func_name = _extract_function_name(node, source_bytes)
        if func_name is None:
            continue

        func_ast = ProgramObject(
            root=node,
            source=ast.source,
            language=ast.language,
        )
        complexities[func_name] = _calculate_node_complexity(func_ast)

    return complexities


def calculate_max_function_complexity(ast: ProgramObject) -> int:
    """Calculate the maximum cyclomatic complexity found in any function."""
    complexities = calculate_function_complexities(ast)
    return max(complexities.values()) if complexities else 0


@dataclass(frozen=True)
class FunctionComplexity:
    """A single function's complexity with its source location and scope kind.

    Drives ``ast.max_function_complexity`` location reporting: ``complexity`` is
    computed with the exact same decision-node logic as
    ``calculate_function_complexities`` (so the max here equals the gate metric),
    and the span/kind let agents map a failing gate back to a concrete edit
    target. ``includes_nested`` is always ``True`` because the AST count walks
    the whole function subtree, including nested callables.
    """

    name: str
    qualified_name: str
    kind: str
    start_line: int
    end_line: int
    complexity: int
    includes_nested: bool = True


def _scope_chain(node, source_bytes: bytes) -> list[tuple[str, str]]:
    """Enclosing ``(type, name)`` scopes, outermost first (excludes ``node``)."""
    chain: list[tuple[str, str]] = []
    parent = node.parent
    while parent is not None:
        if parent.type in _SCOPE_NODE_TYPES:
            chain.append(
                (parent.type, _extract_function_name(parent, source_bytes) or "<anon>")
            )
        parent = parent.parent
    chain.reverse()
    return chain


def _is_async(node) -> bool:
    # tree-sitter-python models ``async def`` as a ``function_definition`` with
    # a leading ``async`` child token (not a distinct node type).
    return node.type == "async_function_definition" or any(
        child.type == "async" for child in node.children
    )


def _classify_kind(node, scope_chain: list[tuple[str, str]]) -> str:
    if scope_chain:
        enclosing_type = scope_chain[-1][0]
        if enclosing_type == "class_definition":
            return "method"
        if enclosing_type in _FUNCTION_NODE_TYPES:
            return "closure"
    return "async_function" if _is_async(node) else "function"


def calculate_function_complexity_entries(
    ast: ProgramObject,
) -> list[FunctionComplexity]:
    """Per-function complexity with locations, parallel to the gate metric.

    Same decision-node counting as ``calculate_function_complexities`` but
    retains every callable (including same-named ones) plus its span, dotted
    qualified name, and scope kind.
    """
    source_bytes = ast.source.encode("utf-8")
    entries: list[FunctionComplexity] = []

    for node in ast.nodes_of_type(*_FUNCTION_NODE_TYPES):
        name = _extract_function_name(node, source_bytes)
        if name is None:
            continue
        scope_chain = _scope_chain(node, source_bytes)
        qualified = ".".join([n for _, n in scope_chain] + [name])
        func_ast = ProgramObject(root=node, source=ast.source, language=ast.language)
        entries.append(
            FunctionComplexity(
                name=name,
                qualified_name=qualified,
                kind=_classify_kind(node, scope_chain),
                start_line=node.start_point[0] + 1,
                end_line=node.end_point[0] + 1,
                complexity=_calculate_node_complexity(func_ast),
            )
        )

    return entries
