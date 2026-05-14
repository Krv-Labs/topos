"""
PDG Comparison — profunctor ``D : E × E^op → ℝ`` restricted to the
academic intra-procedural Program Dependence Graph.
==================================================================

Pairwise comparison of two
:class:`~topos.graphs.pdg.object.ProgramDependenceGraph` instances.
PDG-level comparison surfaces whether a refactor preserved the
def-use and predicate-executor wiring even when the AST has been
rewritten.

Signals:

    data_dep_jaccard   : Jaccard similarity over data-dependence edges
    control_dep_jaccard: Jaccard similarity over control-dependence edges
    statement_delta    : signed change in statement-node count
    density_delta      : signed change in normalized dependence density

Edge equality is by the triple ``(source_id, target_id, var)`` for data
edges and ``(source_id, target_id)`` for control edges — i.e. structural
identity using the stable UAST node ids assigned during parsing.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING

from topos.graphs.pdg.object import DependenceKind

if TYPE_CHECKING:
    from topos.graphs.pdg.object import DependenceEdge, ProgramDependenceGraph


@dataclass(frozen=True)
class PDGComparison:
    """Pairwise comparison summary for two program-dependence graphs."""

    data_dep_jaccard: float
    control_dep_jaccard: float
    statement_delta: int
    density_delta: float
    source_metrics: dict[str, float]
    target_metrics: dict[str, float]

    @property
    def changed(self) -> bool:
        return (
            self.data_dep_jaccard < 1.0
            or self.control_dep_jaccard < 1.0
            or self.statement_delta != 0
            or self.density_delta != 0.0
        )


def _data_edge_key(edge: DependenceEdge) -> tuple[str, str, str]:
    return (edge.source, edge.target, edge.var)


def _control_edge_key(edge: DependenceEdge) -> tuple[str, str]:
    return (edge.source, edge.target)


def _jaccard(a: set, b: set) -> float:
    if not a and not b:
        return 1.0
    return len(a & b) / len(a | b)


def data_dep_jaccard(
    source: ProgramDependenceGraph, target: ProgramDependenceGraph
) -> float:
    """Jaccard similarity over (def → use, var) data-dependence triples."""
    a = {
        _data_edge_key(e)
        for e in source.edges
        if e.kind is DependenceKind.DATA
    }
    b = {
        _data_edge_key(e)
        for e in target.edges
        if e.kind is DependenceKind.DATA
    }
    return _jaccard(a, b)


def control_dep_jaccard(
    source: ProgramDependenceGraph, target: ProgramDependenceGraph
) -> float:
    """Jaccard similarity over (predicate → executor) control-dependence pairs."""
    a = {
        _control_edge_key(e)
        for e in source.edges
        if e.kind is DependenceKind.CONTROL
    }
    b = {
        _control_edge_key(e)
        for e in target.edges
        if e.kind is DependenceKind.CONTROL
    }
    return _jaccard(a, b)


def statement_delta(
    source: ProgramDependenceGraph, target: ProgramDependenceGraph
) -> int:
    """Signed change in statement-node count (target − source)."""
    return len(target.statements) - len(source.statements)


def density_delta(
    source: ProgramDependenceGraph, target: ProgramDependenceGraph
) -> float:
    """Signed change in normalized dependence density (target − source)."""
    s = source.metrics()["pdg.density"]
    t = target.metrics()["pdg.density"]
    return t - s


def compare_pdg(
    source: ProgramDependenceGraph, target: ProgramDependenceGraph
) -> PDGComparison:
    """Run the full PDG comparison suite for a single pair of graphs."""
    return PDGComparison(
        data_dep_jaccard=data_dep_jaccard(source, target),
        control_dep_jaccard=control_dep_jaccard(source, target),
        statement_delta=statement_delta(source, target),
        density_delta=density_delta(source, target),
        source_metrics=source.metrics(),
        target_metrics=target.metrics(),
    )
