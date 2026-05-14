"""
MDG Comparison — profunctor ``D : E × E^op → ℝ`` restricted to the
inter-module dependency graph.
====================================================================

Pairwise comparison of two
:class:`~topos.graphs.mdg.object.ModuleDependencyGraph` instances.
MDG comparison answers questions of the form "did this refactor move
the file's architectural position in the codebase?" — coupling, Martin
instability, fan-in/out, and reachable-import depth.

Signals (all ``target − source`` so a positive delta means *more*):

    coupling_delta     : signed change in Ca + Ce
    instability_delta  : signed change in Martin instability ``Ce/(Ca+Ce)``
    fan_in_delta       : signed change in incoming CALLS edges
    fan_out_delta      : signed change in outgoing CALLS edges
    dep_depth_delta    : signed change in longest IMPORTS chain
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from topos.graphs.mdg.object import ModuleDependencyGraph


@dataclass(frozen=True)
class MDGComparison:
    """Pairwise comparison summary for two module-dependency graphs."""

    coupling_delta: float
    instability_delta: float
    fan_in_delta: float
    fan_out_delta: float
    dep_depth_delta: float
    source_metrics: dict[str, float]
    target_metrics: dict[str, float]

    @property
    def changed(self) -> bool:
        return any(
            delta != 0.0
            for delta in (
                self.coupling_delta,
                self.instability_delta,
                self.fan_in_delta,
                self.fan_out_delta,
                self.dep_depth_delta,
            )
        )


def _delta(
    source: ModuleDependencyGraph, target: ModuleDependencyGraph, key: str
) -> float:
    return target.metrics()[key] - source.metrics()[key]


def coupling_delta(
    source: ModuleDependencyGraph, target: ModuleDependencyGraph
) -> float:
    """Signed change in total coupling Ca + Ce (target − source)."""
    return _delta(source, target, "mdg.coupling")


def instability_delta(
    source: ModuleDependencyGraph, target: ModuleDependencyGraph
) -> float:
    """Signed change in Martin instability ``Ce / (Ca + Ce)``."""
    return _delta(source, target, "mdg.instability")


def fan_in_delta(source: ModuleDependencyGraph, target: ModuleDependencyGraph) -> float:
    """Signed change in incoming CALLS edges."""
    return _delta(source, target, "mdg.fan_in")


def fan_out_delta(
    source: ModuleDependencyGraph, target: ModuleDependencyGraph
) -> float:
    """Signed change in outgoing CALLS edges."""
    return _delta(source, target, "mdg.fan_out")


def dep_depth_delta(
    source: ModuleDependencyGraph, target: ModuleDependencyGraph
) -> float:
    """Signed change in longest IMPORTS chain length."""
    return _delta(source, target, "mdg.dep_depth")


def compare_mdg(
    source: ModuleDependencyGraph, target: ModuleDependencyGraph
) -> MDGComparison:
    """Run the full MDG comparison suite for a single pair of graphs."""
    src_metrics = source.metrics()
    tgt_metrics = target.metrics()
    return MDGComparison(
        coupling_delta=tgt_metrics["mdg.coupling"] - src_metrics["mdg.coupling"],
        instability_delta=tgt_metrics["mdg.instability"]
        - src_metrics["mdg.instability"],
        fan_in_delta=tgt_metrics["mdg.fan_in"] - src_metrics["mdg.fan_in"],
        fan_out_delta=tgt_metrics["mdg.fan_out"] - src_metrics["mdg.fan_out"],
        dep_depth_delta=tgt_metrics["mdg.dep_depth"] - src_metrics["mdg.dep_depth"],
        source_metrics=src_metrics,
        target_metrics=tgt_metrics,
    )
