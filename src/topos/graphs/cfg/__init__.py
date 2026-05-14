"""
Control flow graph (CFG) representation of a program.

The CFG translational functor R_CFG : Lang -> E maps source code to an
intra-procedural control-flow graph built on top of the language-independent
UAST.  It feeds the SIMPLE generator of the free Heyting algebra H(G_qual).
"""

from topos.graphs.cfg.builder import build_cfg_from_uast
from topos.graphs.cfg.models import BasicBlock, CFGEdge, EdgeKind
from topos.graphs.cfg.object import ControlFlowGraph

__all__ = [
    "BasicBlock",
    "CFGEdge",
    "EdgeKind",
    "ControlFlowGraph",
    "build_cfg_from_uast",
]
