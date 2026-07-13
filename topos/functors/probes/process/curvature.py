"""
Process Graph Curvature Probe
-------------------------------
Applies directed Forman-Ricci curvature (Samal et al.) to GitNexus process
graphs to find execution "choke points": single transitions where many
independent call paths squeeze through.

Mathematical Inspiration:
    For a directed edge e = (u -> v):

        Ric(e) = w_e * ( w_u/w_e + w_v/w_e
                         - sum_{e_in ~ u} sqrt(w_u / w_e_in)
                         - sum_{e_out ~ v} sqrt(w_v / w_e_out) )

    Highly negative curvature flags a transition where many independent
    incoming paths (high in-degree at u) and/or many independent outgoing
    paths (high out-degree at v) all funnel through this one edge — the
    signal issue #86 calls a "choke point". Node/edge weights default to
    uniform 1.0 (no call-frequency or timing data exists in GitNexus's
    schema today); this degenerates the formula to unweighted directed
    Forman curvature while keeping the weighted shape for future data.

This is purely advisory — process-graph curvature never influences
SIMPLE/COMPOSABLE/SECURE scoring; it only feeds ``topos refactor process``.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from topos.graphs.process.object import ProcessGraph


@dataclass
class ProcessCurvatureResult:
    """Curvature per process-graph transition.

    Attributes:
        edges: ``(source_node_id, target_node_id, curvature)`` tuples,
            sorted ascending by curvature (most negative — the strongest
            "choke point" signal — first).
    """

    edges: list[tuple[str, str, float]] = field(default_factory=list)


def calculate_process_curvature(
    graph: ProcessGraph,
    node_weights: dict[str, float] | None = None,
) -> ProcessCurvatureResult:
    """
    Compute directed Forman-Ricci curvature for every transition in `graph`.

    Interns string node ids to the dense integers the Rust engine expects,
    calls ``directed_forman_curvature``, then de-interns the results.

    Args:
        graph: The process graph (typically already filtered to paths
            touching a target file via :meth:`ProcessGraph.paths_touching_file`).
        node_weights: Optional per-node weight override, keyed by node id.
            Defaults to uniform 1.0 for every node.

    Returns:
        A :class:`ProcessCurvatureResult` ranked most-negative-curvature first.
    """
    from topos.topos_functors import WeightedEdge, directed_forman_curvature

    edges = graph.edges()
    if not edges:
        return ProcessCurvatureResult(edges=[])

    node_ids: list[str] = []
    id_to_idx: dict[str, int] = {}
    for source, target in edges:
        for node_id in (source, target):
            if node_id not in id_to_idx:
                id_to_idx[node_id] = len(node_ids)
                node_ids.append(node_id)

    idx_weights: dict[int, float] = {}
    if node_weights:
        for node_id, idx in id_to_idx.items():
            if node_id in node_weights:
                idx_weights[idx] = node_weights[node_id]

    weighted_edges = [
        WeightedEdge(id_to_idx[source], id_to_idx[target], 1.0)
        for source, target in edges
    ]
    curvatures = directed_forman_curvature(
        weighted_edges, idx_weights if idx_weights else None
    )

    result_edges = [
        (node_ids[c.source], node_ids[c.target], c.curvature) for c in curvatures
    ]
    result_edges.sort(key=lambda e: e[2])
    return ProcessCurvatureResult(edges=result_edges)
