"""
Per-Function Complexity Analysis
--------------------------------

Provides a function-level breakdown of complexity using the AST.
This is separate from the CFG-based module-level cyclomatic complexity used
in the main program evaluation (SIMPLE generator).

The name/span-aware entries below are computed in Rust directly against
the tree-sitter tree retained by ``ProgramObject`` (Python-specific, same
scope as the logic it replaces — see
:mod:`topos.functors.probes.ast.entropy` for the language-neutral UAST
metrics used elsewhere).
"""

from __future__ import annotations

from topos.core.object import ProgramObject
from topos.topos_functors import (
    FunctionComplexity,
    calculate_function_complexity_entries,
)

__all__ = [
    "FunctionComplexity",
    "calculate_function_complexities",
    "calculate_function_complexity_entries",
    "calculate_max_function_complexity",
]


def calculate_function_complexities(ast: ProgramObject) -> dict[str, int]:
    """Cyclomatic complexity for each function in the AST, keyed by name."""
    return {
        entry.name: entry.complexity
        for entry in calculate_function_complexity_entries(ast)
    }


def calculate_max_function_complexity(ast: ProgramObject) -> int:
    """Calculate the maximum cyclomatic complexity found in any function."""
    complexities = calculate_function_complexities(ast)
    return max(complexities.values()) if complexities else 0
