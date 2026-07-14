"""
CPG Builder
-----------

Construct a CodePropertyGraph from a UAST root by composing the AST, CFG,
DDG, and CDG edge families into a single labeled multigraph
(Yamaguchi et al., arxiv:1909.03496).

Algorithm
=========
1. AST family: every parent → child link from the UAST.
2. CFG family: every edge of the ControlFlowGraph, projected from
   block-level (BB -> BB) onto statement-level (last UAST stmt of source
   block → first UAST stmt of target block).
3. DDG family: each ``DependenceEdge(kind=DATA)`` from the academic PDG.
4. CDG family: each ``DependenceEdge(kind=CONTROL)`` from the academic PDG.
"""

from __future__ import annotations

from topos.graphs.cfg.object import ControlFlowGraph
from topos.graphs.cpg.models import CPGEdge, CPGEdgeKind, CPGNode
from topos.graphs.pdg.object import (
    DependenceKind,
    ProgramDependenceGraph,
)
from topos.graphs.uast.models import UASTNode


def build_cpg(
    uast_root: UASTNode,
    cfg: ControlFlowGraph | None = None,
    pdg: ProgramDependenceGraph | None = None,
    source: str = "",
) -> tuple[dict[str, CPGNode], list[CPGEdge]]:
    """Return ``(nodes, edges)`` for the CPG.

    Reuses pre-built CFG / PDG when supplied; otherwise builds them.
    ``source`` (when given) is forwarded to a freshly-built PDG so its
    data-dependence pass can recover real identifier text instead of
    falling back to per-occurrence node ids (see
    ``topos.graphs.pdg.object._identifier_name``).
    """
    if cfg is None:
        cfg = ControlFlowGraph.from_uast(uast_root)
    if pdg is None:
        pdg = ProgramDependenceGraph.from_uast(uast_root, source=source)

    nodes: dict[str, CPGNode] = {}
    _collect_nodes(uast_root, nodes)

    edges: list[CPGEdge] = []
    edges.extend(_ast_edges(uast_root))
    edges.extend(_cfg_edges(cfg))
    edges.extend(_dependence_edges(pdg))

    return nodes, edges


def _collect_nodes(root: UASTNode, out: dict[str, CPGNode]) -> None:
    stack = [root]
    while stack:
        node = stack.pop()
        key = node.id or f"anon::{id(node):x}"
        if key in out:
            continue
        out[key] = CPGNode(uast=node)
        stack.extend(node.children)


def _ast_edges(root: UASTNode) -> list[CPGEdge]:
    edges: list[CPGEdge] = []
    stack = [root]
    while stack:
        parent = stack.pop()
        parent_id = parent.id or f"anon::{id(parent):x}"
        for child in parent.children:
            child_id = child.id or f"anon::{id(child):x}"
            edges.append(
                CPGEdge(source=parent_id, target=child_id, kind=CPGEdgeKind.AST)
            )
            stack.append(child)
    return edges


def _cfg_edges(cfg: ControlFlowGraph) -> list[CPGEdge]:
    """Project block-level CFG edges down to UAST-statement-level."""
    edges: list[CPGEdge] = []
    for edge in cfg.edges:
        src_block = cfg.blocks.get(edge.source)
        dst_block = cfg.blocks.get(edge.target)
        if src_block is None or dst_block is None:
            continue
        src_stmt = src_block.statements[-1] if src_block.statements else None
        dst_stmt = dst_block.statements[0] if dst_block.statements else None
        if src_stmt is None or dst_stmt is None:
            continue
        edges.append(
            CPGEdge(
                source=src_stmt.id or "<anon>",
                target=dst_stmt.id or "<anon>",
                kind=CPGEdgeKind.CFG,
                label=str(edge.kind),
            )
        )
    return edges


def _dependence_edges(pdg: ProgramDependenceGraph) -> list[CPGEdge]:
    edges: list[CPGEdge] = []
    for dep in pdg.edges:
        kind = CPGEdgeKind.DDG if dep.kind is DependenceKind.DATA else CPGEdgeKind.CDG
        edges.append(
            CPGEdge(
                source=dep.source,
                target=dep.target,
                kind=kind,
                label=dep.var,
            )
        )
    return edges
