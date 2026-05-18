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


def _get_rust_cfg(cfg: ControlFlowGraph):
    from topos.topos_functors import (
        BasicBlock as RustBasicBlock,
        CFGEdge as RustCFGEdge,
        ControlFlowGraph as RustCFG,
        EdgeKind as RustEdgeKind,
        NativeRef as RustNativeRef,
        SourceSpan as RustSourceSpan,
        UASTNode as RustUASTNode,
    )

    # Convert Python EdgeKind to Rust EdgeKind
    _EDGE_KIND_MAP = {
        "unconditional": RustEdgeKind.UNCONDITIONAL,
        "true": RustEdgeKind.TRUE,
        "false": RustEdgeKind.FALSE,
        "loop_back": RustEdgeKind.LOOPBACK,
        "break": RustEdgeKind.BREAK,
        "continue": RustEdgeKind.CONTINUE,
        "return": RustEdgeKind.RETURN,
        "exception": RustEdgeKind.EXCEPTION,
        "switch_case": RustEdgeKind.SWITCHCASE,
    }

    def convert_uast(node):
        return RustUASTNode(
            kind=node.kind,
            lang=node.lang,
            span=RustSourceSpan(
                file=node.span.file,
                start_byte=node.span.start_byte,
                end_byte=node.span.end_byte,
                start_line=node.span.start_line,
                start_column=node.span.start_column,
                end_line=node.span.end_line,
                end_column=node.span.end_column,
            ),
            native=RustNativeRef(
                parser=node.native.parser,
                parser_version=node.native.parser_version,
                node_kind=node.native.node_kind,
            ),
            attributes={k: str(v) for k, v in node.attributes.items()},
            children=[convert_uast(c) for c in node.children],
            id=node.id,
        )

    rust_blocks = {
        bid: RustBasicBlock(
            id=bid,
            statements=[convert_uast(s) for s in block.statements],
            label=block.label,
        )
        for bid, block in cfg.blocks.items()
    }
    rust_edges = [
        RustCFGEdge(
            source=edge.source,
            target=edge.target,
            kind=_EDGE_KIND_MAP[edge.kind],
        )
        for edge in cfg.edges
    ]

    return RustCFG(
        blocks=rust_blocks,
        edges=rust_edges,
        entry_id=cfg.entry_id,
        exit_id=cfg.exit_id,
    )


def cyclomatic_complexity(cfg: ControlFlowGraph) -> int:
    """
    McCabe cyclomatic complexity = E - N + 2P.

    With P = 1 (single connected component, guaranteed by the builder via
    the entry/exit synthetic blocks), this equals E - N + 2.

    A function with no branches yields exactly 1.
    """
    return _get_rust_cfg(cfg).cyclomatic_complexity()


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
    return _get_rust_cfg(cfg).essential_complexity()


def max_nesting_depth(cfg: ControlFlowGraph) -> int:
    """
    Maximum static nesting depth via longest path from entry to any block,
    walking only TRUE / SWITCH_CASE forward edges.  A flat function returns 0.
    """
    return _get_rust_cfg(cfg).max_nesting_depth()
