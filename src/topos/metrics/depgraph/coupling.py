"""
Coupling Metrics
----------------
Quantifies the structural coupling of a module within the dependency graph.

Mathematical Inspiration:
    Robert C. Martin's package coupling metrics measure how tangled a
    module is with its neighbours:

    - **Afferent coupling (Ca)**: number of external modules that depend
      on this module (incoming IMPORTS edges).
    - **Efferent coupling (Ce)**: number of external modules this module
      depends on (outgoing IMPORTS edges).
    - **Instability I = Ce / (Ca + Ce)**: ranges from 0 (maximally
      stable, everyone depends on you) to 1 (maximally unstable, you
      depend on everyone).

    High total coupling (Ca + Ce) indicates a module that is hard to
    change in isolation.  Extreme instability or stability *combined*
    with high coupling is a design-smell.

    Dependency depth measures the longest chain of transitive IMPORTS
    reachable from the module -- deep chains amplify the blast radius
    of changes.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from topos.representations.depgraph.graph import DependencyGraph


@dataclass
class CouplingResult:
    """
    Coupling metrics for a single module.

    Attributes:
        afferent: Number of modules that depend on this one (Ca).
        efferent: Number of modules this one depends on (Ce).
        total: Ca + Ce.
    """

    afferent: int
    efferent: int

    @property
    def total(self) -> int:
        return self.afferent + self.efferent


def calculate_coupling(graph: DependencyGraph, file_node_id: str) -> CouplingResult:
    """
    Calculate afferent and efferent coupling for a file node.

    Counts distinct source/target *File* nodes connected via IMPORTS
    relationships (directly or through contained symbols).

    Args:
        graph: The dependency graph.
        file_node_id: The ID of the file node to analyse.

    Returns:
        A :class:`CouplingResult` with afferent and efferent counts.
    """
    symbol_ids = set(graph.contained_symbols(file_node_id))
    symbol_ids.add(file_node_id)

    efferent_targets: set[str] = set()
    afferent_sources: set[str] = set()

    for sid in symbol_ids:
        for rel in graph.outgoing(sid, "IMPORTS"):
            target_file = _owning_file(graph, rel.target_id)
            if target_file and target_file != file_node_id:
                efferent_targets.add(target_file)

        for rel in graph.incoming(sid, "IMPORTS"):
            source_file = _owning_file(graph, rel.source_id)
            if source_file and source_file != file_node_id:
                afferent_sources.add(source_file)

    return CouplingResult(
        afferent=len(afferent_sources),
        efferent=len(efferent_targets),
    )


def calculate_instability(graph: DependencyGraph, file_node_id: str) -> float:
    """
    Calculate Martin's Instability metric: I = Ce / (Ca + Ce).

    Returns 0.5 when the module has zero coupling (no signal).
    """
    result = calculate_coupling(graph, file_node_id)
    if result.total == 0:
        return 0.5
    return result.efferent / result.total


def calculate_dependency_depth(graph: DependencyGraph, file_node_id: str) -> int:
    """
    Longest chain of transitive IMPORTS from *file_node_id*.

    Uses BFS to avoid cycles.
    """
    visited: set[str] = set()
    frontier = [(file_node_id, 0)]
    max_depth = 0

    while frontier:
        current, depth = frontier.pop(0)
        if current in visited:
            continue
        visited.add(current)
        max_depth = max(max_depth, depth)

        for rel in graph.outgoing(current, "IMPORTS"):
            target_file = _owning_file(graph, rel.target_id)
            if target_file and target_file not in visited:
                frontier.append((target_file, depth + 1))

    return max_depth


def _owning_file(graph: DependencyGraph, node_id: str) -> str | None:
    """Walk up CONTAINS edges to find the File node that owns *node_id*."""
    node = graph.get_node(node_id)
    if node is None:
        return None
    if node.label == "File":
        return node.id

    for rel in graph.incoming(node_id, "CONTAINS"):
        owner = graph.get_node(rel.source_id)
        if owner and owner.label == "File":
            return owner.id

    return None
