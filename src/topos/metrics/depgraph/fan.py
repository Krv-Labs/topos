"""
Fan-in / Fan-out Metrics
------------------------
Counts incoming and outgoing CALLS edges for a file and its symbols.

Mathematical Inspiration:
    Fan-in/fan-out is a classic software-engineering measure of module
    connectivity introduced by Henry & Kafura.

    - **Fan-in**: how many other symbols call into this file's symbols.
      High fan-in means the module is widely depended upon.
    - **Fan-out**: how many external symbols this file's symbols call.
      High fan-out means the module has many dependencies.

    The product ``fan_in * fan_out^2`` (the Henry-Kafura complexity) is
    sometimes used as a structural-risk proxy, but we expose the raw
    counts so the evaluation section can apply its own thresholds.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from topos.graphs.depgraph.graph import DependencyGraph


@dataclass
class FanResult:
    """
    Fan-in and fan-out counts for a file.

    Attributes:
        fan_in: Number of distinct external symbols calling into this file.
        fan_out: Number of distinct external symbols called from this file.
    """

    fan_in: int
    fan_out: int


def calculate_fan_in_out(
    graph: DependencyGraph,
    file_node_id: str,
    symbol_ids: set[str] | None = None,
) -> FanResult:
    """
    Calculate fan-in and fan-out for a file node.

    Counts distinct external caller/callee symbols connected via
    ``CALLS`` relationships to any symbol contained in the file.

    Args:
        graph: The dependency graph.
        file_node_id: The ID of the file node to analyse.
        symbol_ids: Pre-computed set of all contained symbol IDs (including
            *file_node_id* itself). Computed from the graph when not provided.

    Returns:
        A :class:`FanResult` with fan-in and fan-out counts.
    """
    if symbol_ids is None:
        symbol_ids = set(graph.all_contained_symbols(file_node_id))
        symbol_ids.add(file_node_id)

    external_callers: set[str] = set()
    external_callees: set[str] = set()

    for sid in symbol_ids:
        for rel in graph.incoming(sid, "CALLS"):
            if rel.source_id not in symbol_ids:
                external_callers.add(rel.source_id)

        for rel in graph.outgoing(sid, "CALLS"):
            if rel.target_id not in symbol_ids:
                external_callees.add(rel.target_id)

    return FanResult(
        fan_in=len(external_callers),
        fan_out=len(external_callees),
    )
