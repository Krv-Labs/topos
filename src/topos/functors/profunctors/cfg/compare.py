"""
CFG Comparison — profunctor ``D : E × E^op → ℝ`` restricted to CFGs.
====================================================================

Pairwise comparison of two :class:`~topos.graphs.cfg.object.ControlFlowGraph`
instances.  The CFG captures intra-procedural control flow; comparing two
CFGs lets us reason about how a refactor changed branching shape *without*
being misled by lexical churn (whitespace, renames) the AST distance
picks up.

Three orthogonal signals are exposed:

    cyclomatic_delta   : signed change in McCabe complexity
    edge_kind_l1       : L1 distance between edge-kind histograms
                          (a single number in ``[0, 2]``)
    longest_path_delta : signed change in longest acyclic entry→exit path

A composite :class:`CFGComparison` packages all three plus the raw
per-side measurements so callers can render whatever summary they need.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from topos.graphs.cfg.object import ControlFlowGraph


@dataclass(frozen=True)
class CFGComparison:
    """Pairwise comparison summary for two control-flow graphs."""

    cyclomatic_delta: int
    edge_kind_l1: float
    longest_path_delta: int
    source_metrics: dict[str, float]
    target_metrics: dict[str, float]

    @property
    def changed(self) -> bool:
        """True iff any signal reports a non-zero divergence."""
        return (
            self.cyclomatic_delta != 0
            or self.edge_kind_l1 > 0.0
            or self.longest_path_delta != 0
        )


def cyclomatic_delta(source: ControlFlowGraph, target: ControlFlowGraph) -> int:
    """Signed change in McCabe cyclomatic complexity (target − source)."""
    from topos.functors.probes.cfg.complexity import cyclomatic_complexity

    return cyclomatic_complexity(target) - cyclomatic_complexity(source)


def edge_kind_histogram(cfg: ControlFlowGraph) -> dict[str, int]:
    """Count edges grouped by :class:`EdgeKind`."""
    histogram: dict[str, int] = {}
    for edge in cfg.edges:
        key = str(edge.kind)
        histogram[key] = histogram.get(key, 0) + 1
    return histogram


def edge_kind_l1_distance(
    source: ControlFlowGraph, target: ControlFlowGraph
) -> float:
    """
    L1 distance between the two edge-kind histograms, normalized to
    probability distributions.  Result lies in ``[0, 1]`` (half of the
    raw L1, matching the convention used elsewhere in the codebase).
    """
    a = edge_kind_histogram(source)
    b = edge_kind_histogram(target)
    total_a = sum(a.values()) or 1
    total_b = sum(b.values()) or 1
    kinds = set(a) | set(b)
    l1 = sum(abs(a.get(k, 0) / total_a - b.get(k, 0) / total_b) for k in kinds)
    return l1 / 2.0


def longest_path_delta(
    source: ControlFlowGraph, target: ControlFlowGraph
) -> int:
    """Signed change in longest acyclic entry→exit path length."""
    from topos.functors.probes.cfg.paths import longest_acyclic_path

    return longest_acyclic_path(target) - longest_acyclic_path(source)


def compare_cfg(
    source: ControlFlowGraph, target: ControlFlowGraph
) -> CFGComparison:
    """Run the full CFG comparison suite for a single pair of graphs."""
    return CFGComparison(
        cyclomatic_delta=cyclomatic_delta(source, target),
        edge_kind_l1=edge_kind_l1_distance(source, target),
        longest_path_delta=longest_path_delta(source, target),
        source_metrics=source.metrics(),
        target_metrics=target.metrics(),
    )
