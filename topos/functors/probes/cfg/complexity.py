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
    # `ProgramMorphism.build_cfg()` already returns a Rust-native CFG
    # (`topos.topos_functors.CoreControlFlowGraph`) with these methods
    # built in — nothing to convert. Only the plain, pure-Python container
    # from `topos.graphs.cfg.builder.build_cfg_from_uast` (used by the
    # regression-diff subtree-CFG path) needs converting below.
    if hasattr(cfg, "cyclomatic_complexity"):
        return cfg

    if hasattr(cfg, "_rust_cfg") and cfg._rust_cfg is not None:
        return cfg._rust_cfg

    from topos.topos_functors import (
        BasicBlock as RustBasicBlock,
    )
    from topos.topos_functors import (
        CFGEdge as RustCFGEdge,
    )
    from topos.topos_functors import (
        ControlFlowGraph as RustCFG,
    )
    from topos.topos_functors import (
        EdgeKind as RustEdgeKind,
    )
    from topos.topos_functors import (
        NativeRef as RustNativeRef,
    )
    from topos.topos_functors import (
        SourceSpan as RustSourceSpan,
    )
    from topos.topos_functors import (
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

    rust_cfg = RustCFG(
        blocks=rust_blocks,
        edges=rust_edges,
        entry_id=cfg.entry_id,
        exit_id=cfg.exit_id,
    )
    # Cache for subsequent calls
    cfg._rust_cfg = rust_cfg
    return rust_cfg


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
