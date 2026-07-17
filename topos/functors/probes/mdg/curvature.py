"""
MDG Curvature Probe
--------------------
Applies balanced Forman curvature (Topping, Di Giovanni, Chamberlain, Dong &
Bronstein, "Understanding over-squashing and bottlenecks on graphs via
curvature", ICLR 2022, arxiv:2111.14522) to the file-level dependency graph,
to name concrete edges worth strengthening.

Mathematical Inspiration:
    The paper uses this curvature to find GNN message-passing bottlenecks —
    edges with very negative curvature "squash" information from
    exponentially many distant neighborhoods through a single transition.
    The same signal, applied to module dependencies instead of message
    passing, flags load-bearing import edges: highly negative curvature
    means many otherwise-unrelated modules route their coupling through
    this one dependency.

This is purely advisory — it is never folded into ``mdg.*`` metrics or the
COMPOSABLE score; it only feeds ``topos refactor dependencies``.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import TYPE_CHECKING

from topos.functors.probes.mdg.coupling import _owning_file

if TYPE_CHECKING:
    from topos.graphs.mdg.object import ModuleDependencyGraph, RelationshipType


@dataclass
class MdgCurvatureResult:
    """Curvature per file-level dependency edge touching the target file.

    Attributes:
        edges: ``(source_file_id, target_file_id, curvature)`` tuples,
            sorted ascending by curvature (most negative — the strongest
            "strengthen this" signal — first).
    """

    edges: list[tuple[str, str, float]] = field(default_factory=list)


def calculate_mdg_curvature(
    graph: ModuleDependencyGraph,
    file_node_id: str,
    rel_type: RelationshipType = "IMPORTS",
) -> MdgCurvatureResult:
    """
    Compute balanced Forman curvature for every dependency edge touching
    ``file_node_id``.

    Builds the whole project's file-level dependency graph (resolving
    symbol-level `rel_type` edges, e.g. IMPORTS, up to their owning File
    nodes, matching :mod:`topos.functors.probes.mdg.coupling`'s approach) so
    that curvature at each edge reflects its true local neighborhood rather
    than a truncated one-file ego network, then filters the result down to
    edges incident to ``file_node_id``.

    Args:
        graph: The dependency graph.
        file_node_id: The ID of the file node to analyze.
        rel_type: Relationship type defining "dependency" edges.

    Returns:
        An :class:`MdgCurvatureResult` ranked most-negative-curvature first.
    """
    from topos.topos_functors import WeightedEdge, balanced_forman_curvature

    file_ids = [n.id for n in graph.nodes.values() if n.label == "File"]
    id_to_idx = {fid: i for i, fid in enumerate(file_ids)}
    if file_node_id not in id_to_idx:
        return MdgCurvatureResult(edges=[])

    edge_pairs: set[tuple[int, int]] = set()
    for fid in file_ids:
        symbol_ids = set(graph.all_contained_symbols(fid))
        symbol_ids.add(fid)
        for sid in symbol_ids:
            for rel in graph.outgoing(sid, rel_type):
                target_file = _owning_file(graph, rel.target_id)
                if target_file and target_file != fid and target_file in id_to_idx:
                    a, b = id_to_idx[fid], id_to_idx[target_file]
                    edge_pairs.add((min(a, b), max(a, b)))

    weighted_edges = [WeightedEdge(a, b, 1.0) for a, b in edge_pairs]
    curvatures = balanced_forman_curvature(weighted_edges)

    target_idx = id_to_idx[file_node_id]
    edges = [
        (file_ids[c.source], file_ids[c.target], c.curvature)
        for c in curvatures
        if c.source == target_idx or c.target == target_idx
    ]
    edges.sort(key=lambda e: e[2])
    return MdgCurvatureResult(edges=edges)
