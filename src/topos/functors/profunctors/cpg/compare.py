"""
CPG Comparison — profunctor ``D : E × E^op → ℝ`` restricted to the
Code Property Graph.
====================================================================

Pairwise comparison of two
:class:`~topos.graphs.cpg.object.CodePropertyGraph` instances.  The CPG
fuses AST ∪ CFG ∪ DDG ∪ CDG into a single labeled multigraph; comparing
two CPGs gives a single end-to-end signal for "did this refactor change
the program's semantic structure?".

Signals:

    family_jaccards   : Jaccard similarity per edge family
                        ({AST, CFG, DDG, CDG} → float in [0, 1])
    node_jaccard      : Jaccard similarity over CPG node ids
    dangerous_delta   : signed change in count of dangerous-API call sites
    taint_delta       : signed change in count of source → sink taint paths
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import TYPE_CHECKING

from topos.graphs.cpg.models import CPGEdgeKind

if TYPE_CHECKING:
    from topos.graphs.cpg.object import CodePropertyGraph


@dataclass(frozen=True)
class CPGComparison:
    """Pairwise comparison summary for two code-property graphs."""

    family_jaccards: dict[str, float]
    node_jaccard: float
    dangerous_delta: float
    taint_delta: float
    source_metrics: dict[str, float] = field(default_factory=dict)
    target_metrics: dict[str, float] = field(default_factory=dict)

    @property
    def changed(self) -> bool:
        if self.node_jaccard < 1.0:
            return True
        if any(v < 1.0 for v in self.family_jaccards.values()):
            return True
        return self.dangerous_delta != 0.0 or self.taint_delta != 0.0


def _jaccard(a: set, b: set) -> float:
    if not a and not b:
        return 1.0
    return len(a & b) / len(a | b)


def node_jaccard(
    source: CodePropertyGraph, target: CodePropertyGraph
) -> float:
    """Jaccard similarity over CPG node ids (stable UAST node hashes)."""
    return _jaccard(set(source.nodes), set(target.nodes))


def family_jaccards(
    source: CodePropertyGraph, target: CodePropertyGraph
) -> dict[str, float]:
    """
    Jaccard similarity *per edge family* over edge identities.

    Each edge is identified by the triple ``(source, target, label)``
    so that, e.g., two DDG edges over the same variable count as the
    same edge and two CFG edges with different branch labels (``true``
    vs ``false``) count as distinct.  Result keys are the CPGEdgeKind
    enum values (``ast``, ``cfg``, ``ddg``, ``cdg``).
    """
    out: dict[str, float] = {}
    for kind in CPGEdgeKind:
        a = {
            (e.source, e.target, e.label)
            for e in source.edges
            if e.kind is kind
        }
        b = {
            (e.source, e.target, e.label)
            for e in target.edges
            if e.kind is kind
        }
        out[str(kind)] = _jaccard(a, b)
    return out


def dangerous_delta(
    source: CodePropertyGraph, target: CodePropertyGraph
) -> float:
    """Signed change in dangerous-API call-site count (target − source)."""
    return (
        target.metrics()["cpg.dangerous_calls"]
        - source.metrics()["cpg.dangerous_calls"]
    )


def taint_delta(
    source: CodePropertyGraph, target: CodePropertyGraph
) -> float:
    """Signed change in source → sink taint-path count (target − source)."""
    return (
        target.metrics()["cpg.taint_flows"]
        - source.metrics()["cpg.taint_flows"]
    )


def compare_cpg(
    source: CodePropertyGraph, target: CodePropertyGraph
) -> CPGComparison:
    """Run the full CPG comparison suite for a single pair of graphs."""
    return CPGComparison(
        family_jaccards=family_jaccards(source, target),
        node_jaccard=node_jaccard(source, target),
        dangerous_delta=dangerous_delta(source, target),
        taint_delta=taint_delta(source, target),
        source_metrics=source.metrics(),
        target_metrics=target.metrics(),
    )
