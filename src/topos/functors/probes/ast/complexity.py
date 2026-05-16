"""
Per-Function Complexity Analysis
--------------------------------

Provides a function-level breakdown of complexity using the AST.
This is separate from the CFG-based module-level cyclomatic complexity used
in the main program evaluation (SIMPLE generator).
"""

from __future__ import annotations

from topos.core.object import ProgramObject

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
