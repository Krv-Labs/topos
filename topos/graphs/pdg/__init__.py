"""
Program Dependence Graph (PDG) representation — academic, intra-procedural.

Implements the Ferrante/Ottenstein PDG: a graph over a single procedure's
statement nodes with two edge families:

    DDG : data-dependence edges  (def -> use)
    CDG : control-dependence edges (predicate -> branch executor)

This is **not** the inter-module dependency graph fed by GitNexus —
that lives at :mod:`topos.graphs.mdg`.  The PDG is consumed primarily
by the Code Property Graph builder.
"""

from topos.graphs.pdg.object import (
    DependenceEdge,
    DependenceKind,
    ProgramDependenceGraph,
)

__all__ = [
    "ProgramDependenceGraph",
    "DependenceEdge",
    "DependenceKind",
]
