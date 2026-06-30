"""
Balanced Forman-Ricci Curvature over the Module Dependency Graph.

Computes the Topping et al. (ICLR 2022) balanced Forman-Ricci curvature for
every IMPORTS edge in the MDG.  The MDG is directed; edges are symmetrized
before passing to the Rust kernel so the formula matches the paper exactly.

A negative RIC score identifies a *bridge* edge — a dependency with no shared
neighbours between source and target modules.  Bridge edges are architectural
bottlenecks: changing one module forces a change in the other with no
redundant path to absorb the impact.

This is edge-level information that Martin instability cannot express:
Martin scores both bridge edges and redundantly-connected edges of identical
degree identically (they share the same in/out degree ratio).
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from topos.graphs.mdg.object import ModuleDependencyGraph


@dataclass(frozen=True)
class ModuleEdgeCurvature:
    """Balanced Forman-Ricci curvature for one MDG edge."""

    source: str
    target: str
    ric: float
    is_bridge: bool


def mdg_edge_curvatures(
    graph: "ModuleDependencyGraph",
) -> list[ModuleEdgeCurvature]:
    """Compute balanced FRC for all IMPORTS edges among File nodes in *graph*.

    Returns one entry per undirected edge {source, target} (deduped).  The
    directed MDG is symmetrized: both ``A→B`` and ``B→A`` become a single
    undirected edge.  If neither direction exists the edge is absent.

    Returns an empty list when the graph has fewer than two File nodes or no
    IMPORTS relationships between them.
    """
    from topos.topos_functors import calculate_balanced_frc

    # Index all File nodes.
    file_nodes = graph.nodes_of_label("File")
    if len(file_nodes) < 2:
        return []

    node_to_idx: dict[str, int] = {n.id: i for i, n in enumerate(file_nodes)}
    idx_to_path: dict[int, str] = {
        i: str(n.properties.get("filePath", n.id)) for i, n in enumerate(file_nodes)
    }

    # Collect IMPORTS edges between File nodes and symmetrize.
    seen: set[tuple[int, int]] = set()
    undirected_edges: list[tuple[int, int]] = []
    for rel in graph.relationships_of_type("IMPORTS"):
        src_idx = node_to_idx.get(rel.source_id)
        tgt_idx = node_to_idx.get(rel.target_id)
        if src_idx is None or tgt_idx is None or src_idx == tgt_idx:
            continue
        key = (min(src_idx, tgt_idx), max(src_idx, tgt_idx))
        if key not in seen:
            seen.add(key)
            undirected_edges.append(key)

    if not undirected_edges:
        return []

    raw = calculate_balanced_frc(len(file_nodes), undirected_edges)
    return [
        ModuleEdgeCurvature(
            source=idx_to_path[e.source_idx],
            target=idx_to_path[e.target_idx],
            ric=e.ric,
            is_bridge=e.is_bridge,
        )
        for e in raw
    ]
