"""
AST Representation
------------------
Adapts the existing :class:`~topos.core.object.ProgramObject` to the
:class:`~topos.graphs.base.Representation` protocol.

This does **not** replace ``ProgramObject``; it wraps it so the
CharacteristicMorphism can treat it uniformly alongside other
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

    @property
    def dimension(self) -> str:
        # Feeds the SIMPLE generator via ``ast.entropy``.  Cyclomatic
        # complexity is produced by the CFG representation.
        return "simple"

    def metrics(self) -> dict[str, float]:
        from topos.functors.probes.ast.complexity import (
            calculate_max_function_complexity,
        )
        from topos.functors.probes.ast.entropy import calculate_kolmogorov_proxy

        return {
            "ast.entropy": calculate_kolmogorov_proxy(self.source),
            "ast.max_function_complexity": float(
                calculate_max_function_complexity(self.program_object)
            ),
        }
