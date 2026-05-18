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
    from topos.functors.probes.cfg.complexity import _get_rust_cfg

    return _get_rust_cfg(cfg).longest_acyclic_path()
