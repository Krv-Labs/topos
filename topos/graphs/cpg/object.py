"""
CodePropertyGraph Representation.

Implements the ``Representation`` protocol on the SECURE generator.
"""

from __future__ import annotations

from dataclasses import dataclass, field

from topos.graphs.cpg.builder import build_cpg
from topos.graphs.cpg.models import CPGEdge, CPGNode
from topos.graphs.uast.models import UASTNode


@dataclass
class CodePropertyGraph:
    """
    A Code Property Graph (Yamaguchi et al., arxiv:1909.03496).

    Attributes:
        nodes:    UAST nodes keyed by stable id.
        edges:    Labeled CPG edges across the four families {AST, CFG, DDG, CDG}.
        language: The source language (passed through for danger-registry lookup).
    """

    nodes: dict[str, CPGNode] = field(default_factory=dict)
    edges: list[CPGEdge] = field(default_factory=list)
    language: str = "python"
    source: str = ""  # original source text — needed to recover token text from spans

    @property
    def name(self) -> str:
        return "cpg"

    @property
    def dimension(self) -> str:
        return "secure"

    @classmethod
    def from_uast(cls, uast_root: UASTNode, source: str = "") -> CodePropertyGraph:
        nodes, edges = build_cpg(uast_root, source=source)
        return cls(nodes=nodes, edges=edges, language=uast_root.lang, source=source)

    def node_text(self, node: CPGNode) -> str:
        """Slice the original source by a node's byte span."""
        if not self.source:
            return ""
        span = node.uast.span
        # Defensive bounds — source may be a different revision than the parse.
        if span.end_byte > len(self.source.encode("utf-8")):
            return ""
        return self.source.encode("utf-8")[span.start_byte : span.end_byte].decode(
            "utf-8", errors="replace"
        )

    # ------------------------------------------------------------------
    # Queries used by security probes
    # ------------------------------------------------------------------

    def nodes_of_kind(self, kind: str) -> list[CPGNode]:
        return [n for n in self.nodes.values() if n.kind == kind]

    # ------------------------------------------------------------------
    # Metrics
    # ------------------------------------------------------------------

    def metrics(self) -> dict[str, float]:
        from topos.functors.probes.cpg.danger import dangerous_api_reachable
        from topos.functors.probes.cpg.taint import taint_flow_paths

        return {
            "cpg.dangerous_calls": float(dangerous_api_reachable(self)),
            "cpg.taint_flows": float(taint_flow_paths(self)),
        }
