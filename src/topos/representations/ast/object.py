"""
AST Representation
------------------
Adapts the existing :class:`~topos.core.object.ProgramObject` to the
:class:`~topos.representations.base.Representation` protocol.

This does **not** replace ``ProgramObject``; it wraps it so the
SubobjectClassifier can treat it uniformly alongside other
representations (dependency graph, CFG, etc.).

The metrics produced by this representation are:
- ``ast.complexity`` -- cyclomatic complexity
- ``ast.entropy`` -- Kolmogorov proxy via compression ratio
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from topos.core.object import ProgramObject


@dataclass
class ASTRepresentation:
    """
    Representation adapter for Abstract Syntax Trees.

    Wraps a ``ProgramObject`` and its source text, exposing complexity
    and entropy as representation-level metrics.

    Attributes:
        program_object: The underlying parsed AST.
        source: The original source code (needed for entropy).
    """

    program_object: ProgramObject
    source: str

    @property
    def name(self) -> str:
        return "ast"

    def metrics(self) -> dict[str, float]:
        from topos.metrics.ast.complexity import calculate_cyclomatic_complexity
        from topos.metrics.ast.entropy import calculate_kolmogorov_proxy

        return {
            "ast.complexity": float(
                calculate_cyclomatic_complexity(self.program_object)
            ),
            "ast.entropy": calculate_kolmogorov_proxy(self.source),
        }
