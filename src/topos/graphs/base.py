"""
Representation Protocol
-----------------------
Defines the contract that all program representations must satisfy.

In the topos of programs, a program can be viewed through multiple lenses:
its AST structure, its dependency graph, its control-flow graph, etc.
Each lens is a *representation* -- a distinct categorical object that
captures different structural invariants of the same morphism.

Every representation can produce a dictionary of metric values.  These
metrics are routed through representation-specific evaluation sections
and ultimately aggregated by the lattice into a single verdict.
"""

from __future__ import annotations

from typing import Protocol, runtime_checkable


@runtime_checkable
class Representation(Protocol):
    """
    Protocol that all program representations must implement.

    A representation is a structural view of a program (AST, dependency
    graph, CFG, ...) that can be measured along its own metric axes.

    Attributes:
        name: A unique identifier for this representation type
              (e.g. ``"ast"``, ``"depgraph"``).
    """

    @property
    def name(self) -> str: ...

    def metrics(self) -> dict[str, float]:
        """
        Compute all metric values for this representation.

        Returns:
            A dictionary mapping metric names to their raw float values.
            Metric names should be namespaced by representation
            (e.g. ``"ast.complexity"``, ``"depgraph.coupling"``).
        """
        ...
