"""
ControlFlowGraph Representation
-------------------------------

The CFG translational functor's image in the Topos E.  Implements the
``Representation`` protocol and feeds the SIMPLE generator of ℋ.

Metrics emitted (namespace ``cfg.*``):
    cfg.cyclomatic     — McCabe complexity (E - N + 2P, with P = 1).
    cfg.essential      — essential complexity (structured-decomposition reduction).
    cfg.nesting_depth  — maximum static nesting depth.
    cfg.longest_path   — longest acyclic path (a rough proxy for path explosion).
"""

from __future__ import annotations

from dataclasses import dataclass, field

from topos.graphs.cfg.builder import build_cfg_from_uast
from topos.graphs.cfg.models import BasicBlock, CFGEdge
from topos.graphs.uast.models import UASTNode


@dataclass
class ControlFlowGraph:
    """
    A language-independent control-flow graph built on UAST.

    Construct via :meth:`from_uast` for a fully-populated graph, or pass
    pre-computed ``blocks`` and ``edges`` for tests.
    """

    blocks: dict[int, BasicBlock] = field(default_factory=dict)
    edges: list[CFGEdge] = field(default_factory=list)
    entry_id: int = 0
    exit_id: int = 1

    @property
    def name(self) -> str:
        return "cfg"

    @property
    def dimension(self) -> str:
        # SIMPLE generator of H(G_qual).
        return "simple"

    @classmethod
    def from_uast(cls, uast_root: UASTNode) -> ControlFlowGraph:
        """Build a CFG from a UAST root, covering every callable."""
        blocks, edges, entry_id, exit_id = build_cfg_from_uast(uast_root)
        return cls(blocks=blocks, edges=edges, entry_id=entry_id, exit_id=exit_id)

    # ------------------------------------------------------------------
    # Graph queries
    # ------------------------------------------------------------------

    def successors(self, block_id: int) -> list[CFGEdge]:
        return [e for e in self.edges if e.source == block_id]

    def predecessors(self, block_id: int) -> list[CFGEdge]:
        return [e for e in self.edges if e.target == block_id]

    # ------------------------------------------------------------------
    # Metrics
    # ------------------------------------------------------------------

    def metrics(self) -> dict[str, float]:
        from topos.functors.probes.cfg.complexity import (
            cyclomatic_complexity,
            essential_complexity,
            max_nesting_depth,
        )
        from topos.functors.probes.cfg.paths import longest_acyclic_path

        return {
            "cfg.cyclomatic": float(cyclomatic_complexity(self)),
            "cfg.essential": float(essential_complexity(self)),
            "cfg.nesting_depth": float(max_nesting_depth(self)),
            "cfg.longest_path": float(longest_acyclic_path(self)),
        }
