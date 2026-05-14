"""
CFG path probes.
----------------

Path-shape statistics over the ControlFlowGraph.
"""

from __future__ import annotations

from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from topos.graphs.cfg.object import ControlFlowGraph


def longest_acyclic_path(cfg: ControlFlowGraph) -> int:
    """
    Length (in edges) of the longest simple (cycle-free) path from entry
    to exit.  Bounded by the block count so we cap the DFS at that depth.
    """
    # Adjacency list, excluding LOOP_BACK edges so we measure *acyclic* paths.
    from topos.graphs.cfg.models import EdgeKind

    adj: dict[int, list[int]] = {bid: [] for bid in cfg.blocks}
    for edge in cfg.edges:
        if edge.kind is EdgeKind.LOOP_BACK:
            continue
        adj.setdefault(edge.source, []).append(edge.target)

    best = 0
    sys_block_count = len(cfg.blocks)

    def dfs(node: int, length: int, visited: set[int]) -> None:
        nonlocal best
        if node == cfg.exit_id:
            best = max(best, length)
            return
        if length > sys_block_count:
            return
        for nxt in adj.get(node, []):
            if nxt in visited:
                continue
            visited.add(nxt)
            dfs(nxt, length + 1, visited)
            visited.remove(nxt)

    dfs(cfg.entry_id, 0, {cfg.entry_id})
    return best
