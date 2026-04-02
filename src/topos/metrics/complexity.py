"""
Complexity Module
-----------------
Quantifies the 'Density' of the morphism.

Mathematical Inspiration:
    Cyclomatic complexity represents the number of linearly independent
    paths through a program's control flow graph. In our Topos, this acts
    as a measure of 'Logical Entropy'.

    For a control flow graph G with:
    - E edges
    - N nodes
    - P connected components

    Cyclomatic Complexity M = E - N + 2P

    Equivalently, M = (number of decision points) + 1

    High complexity in commodity code suggests the morphism is
    'logic-heavy' but 'truth-poor'—many paths, but questionable
    structural integrity.

    We compute this directly from the AST by counting decision nodes
    (if, for, while, except, etc.) rather than constructing the full CFG.
"""

from __future__ import annotations

from typing import TYPE_CHECKING

if TYPE_CHECKING:
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


def calculate_cyclomatic_complexity(ast: ProgramObject) -> int:
    """
    Measures the branching manifold of the program.

    Computes cyclomatic complexity by counting decision points in the AST.
    Each decision point adds one to the base complexity of 1.

    Args:
        ast: The ProgramObject (parsed AST) to analyze.

    Returns:
        The cyclomatic complexity score (minimum 1).

    Example:
        A simple function with no branches: M = 1
        A function with one if-else: M = 2
        A function with nested if-else: M = 3+

    Mathematical Note:
        This is an approximation. True cyclomatic complexity requires
        analyzing the control flow graph, but AST-based counting provides
        a reasonable proxy for most Python code.
    """
    decision_count = 0

    for node in ast.traverse():
        if node.type in DECISION_NODE_TYPES:
            decision_count += 1
            if node.type == "boolean_operator":
                decision_count += _count_boolean_operators(node) - 1

    return decision_count + 1


def _count_boolean_operators(node) -> int:
    """
    Count chained boolean operators (and/or).

    Each boolean operator in a chain adds a decision point:
    `a and b and c` has 2 decision points.
    """
    count = 1
    for child in node.children:
        if child.type == "boolean_operator":
            count += _count_boolean_operators(child)
    return count


def calculate_function_complexities(ast: ProgramObject) -> dict[str, int]:
    """
    Calculate cyclomatic complexity for each function in the AST.

    Args:
        ast: The ProgramObject to analyze.

    Returns:
        A dictionary mapping function names to their complexity scores.
    """
    from topos.core.object import ProgramObject

    complexities: dict[str, int] = {}
    source_bytes = ast.source.encode("utf-8")

    for node in ast.nodes_of_type("function_definition", "async_function_definition"):
        name_node = None
        for child in node.children:
            if child.type == "identifier":
                name_node = child
                break

        if name_node is None:
            continue

        func_name = source_bytes[name_node.start_byte : name_node.end_byte].decode(
            "utf-8"
        )

        func_ast = ProgramObject(
            root=node,
            source=ast.source,
            language=ast.language,
        )
        complexities[func_name] = calculate_cyclomatic_complexity(func_ast)

    return complexities


def calculate_average_complexity(ast: ProgramObject) -> float:
    """
    Calculate the average complexity across all functions.

    Args:
        ast: The ProgramObject to analyze.

    Returns:
        The mean cyclomatic complexity of all functions,
        or 1.0 if no functions are defined.
    """
    complexities = calculate_function_complexities(ast)

    if not complexities:
        return 1.0

    return sum(complexities.values()) / len(complexities)
