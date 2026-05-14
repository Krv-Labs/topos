"""
CFG complexity probes.
----------------------

McCabe cyclomatic complexity (E - N + 2P) and structural derivatives,
computed directly on the ControlFlowGraph.  The CFG builder guarantees a
single connected component (P = 1) so the formula reduces to E - N + 2.
"""

from __future__ import annotations

from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from topos.graphs.cfg.object import ControlFlowGraph


def cyclomatic_complexity(cfg: ControlFlowGraph) -> int:
    """
    McCabe cyclomatic complexity = E - N + 2P.

    With P = 1 (single connected component, guaranteed by the builder via
    the entry/exit synthetic blocks), this equals E - N + 2.

    A function with no branches yields exactly 1.
    """
    n = len(cfg.blocks)
    e = len(cfg.edges)
    p = _connected_components(cfg)
    return max(1, e - n + 2 * p)


def essential_complexity(cfg: ControlFlowGraph) -> int:
    """
    Essential complexity (Cabe 1989): cyclomatic complexity after iteratively
    collapsing every D-structured primitive (single-entry single-exit
    decision/loop/switch substructure).

    Implementation note: a full structured-decomposition pass is non-trivial.
    We approximate by counting decision blocks whose successors *do not*
    converge cleanly to a single join — i.e. blocks that issue a
    ``BREAK`` / ``CONTINUE`` / ``RETURN`` mid-substructure.  These are the
    "unstructured" branches McCabe's metric is built to surface.
    """
    from topos.graphs.cfg.models import EdgeKind

    unstructured = 0
    for edge in cfg.edges:
        if edge.kind in {EdgeKind.BREAK, EdgeKind.CONTINUE, EdgeKind.RETURN}:
            # Each non-fall-through exit out of a structured region adds
            # one to essential complexity.
            unstructured += 1
    return max(1, unstructured + 1)


def max_nesting_depth(cfg: ControlFlowGraph) -> int:
    """
    Maximum static nesting depth via longest path from entry to any block,
    walking only TRUE / SWITCH_CASE forward edges.  A flat function returns 0.
    """
    from topos.graphs.cfg.models import EdgeKind

    nesting_edge_kinds = {EdgeKind.TRUE, EdgeKind.SWITCH_CASE}
    depth: dict[int, int] = {cfg.entry_id: 0}
    # Topological-ish traversal — fine for our finite CFGs.
    changed = True
    iterations = 0
    while changed and iterations < len(cfg.blocks) * 2:
        changed = False
        for edge in cfg.edges:
            if edge.source not in depth:
                continue
            inc = 1 if edge.kind in nesting_edge_kinds else 0
            candidate = depth[edge.source] + inc
            if candidate > depth.get(edge.target, -1):
                depth[edge.target] = candidate
                changed = True
        iterations += 1
    return max(depth.values()) if depth else 0


# ---------------------------------------------------------------------------
# Private helpers
# ---------------------------------------------------------------------------


def _connected_components(cfg: ControlFlowGraph) -> int:
    """Count connected components in the undirected projection of the CFG."""
    parent: dict[int, int] = {bid: bid for bid in cfg.blocks}

    def find(x: int) -> int:
        while parent[x] != x:
            parent[x] = parent[parent[x]]
            x = parent[x]
        return x

    def union(a: int, b: int) -> None:
        ra, rb = find(a), find(b)
        if ra != rb:
            parent[ra] = rb

    for edge in cfg.edges:
        if edge.source in parent and edge.target in parent:
            union(edge.source, edge.target)

    roots = {find(b) for b in parent}
    return len(roots) or 1
