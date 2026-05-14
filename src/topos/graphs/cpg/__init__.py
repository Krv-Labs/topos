"""
Code Property Graph (CPG) representation.

Implements the Yamaguchi et al. construction (arxiv:1909.03496):
a single labeled multigraph whose nodes are the UAST nodes of a program
and whose edges come from four disjoint families:

    AST  — parent → child structural edges
    CFG  — control-flow successor edges
    DDG  — data-dependence edges (a definition reaches a use)
    CDG  — control-dependence edges (the executor of v branches on u)

The CPG is the translational functor R_CPG : Lang → E whose probes feed
the SECURE generator of H(G_qual).
"""

from topos.graphs.cpg.builder import build_cpg
from topos.graphs.cpg.models import CPGEdge, CPGEdgeKind, CPGNode
from topos.graphs.cpg.object import CodePropertyGraph

__all__ = [
    "CPGNode",
    "CPGEdge",
    "CPGEdgeKind",
    "CodePropertyGraph",
    "build_cpg",
]
