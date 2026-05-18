"""
CPG Models — Code Property Graph node & edge types per Yamaguchi et al.
"""

from __future__ import annotations

from dataclasses import dataclass
from enum import StrEnum

from topos.graphs.uast.models import UASTNode


class CPGEdgeKind(StrEnum):
    """The four CPG edge families."""

    AST = "ast"  # parent → child
    CFG = "cfg"  # control-flow successor
    DDG = "ddg"  # data dependence (def → use)
    CDG = "cdg"  # control dependence (predicate → executor)


@dataclass(frozen=True)
class CPGEdge:
    """A typed, labeled edge in the CPG multigraph."""

    source: str  # source node id (UAST node id)
    target: str
    kind: CPGEdgeKind
    label: str = ""  # variable name for DDG, branch label for CFG, ...


@dataclass
class CPGNode:
    """
    A CPG node: a UAST node enriched with quick-lookup metadata.

    The CPG uses the UAST node directly as the node payload — every CPG
    node *is* a UAST node, so the AST family of edges is implicit in the
    UAST ``children`` lists.  We materialize them as CPGEdges anyway so
    downstream queries are uniform.
    """

    uast: UASTNode

    @property
    def id(self) -> str:
        return self.uast.id

    @property
    def kind(self) -> str:
        return self.uast.kind

    @property
    def attributes(self) -> dict:
        return self.uast.attributes
