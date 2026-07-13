"""ControlFlowGraph — plain container for the UAST-subtree CFG builder.

Distinct from the topos-core-backed graph returned by
``ProgramMorphism.build_cfg()`` (``topos.topos_functors.CoreControlFlowGraph``,
used by the main evaluate/inspect pipeline) — this is a lightweight,
pure-Python container built directly from
:func:`topos.graphs.cfg.builder.build_cfg_from_uast` for ad-hoc CFGs over an
arbitrary UAST subtree (e.g. one function), used by the regression-diff
tooling in :mod:`topos.mcp.tools.assess.render`. Complexity computation still
delegates to Rust via
:func:`topos.functors.probes.cfg.complexity.cyclomatic_complexity`.
"""

from __future__ import annotations

from dataclasses import dataclass, field

from topos.graphs.cfg.models import BasicBlock, CFGEdge

__all__ = ["ControlFlowGraph"]


@dataclass
class ControlFlowGraph:
    """Blocks/edges from :func:`build_cfg_from_uast`, plus entry/exit ids."""

    blocks: dict[int, BasicBlock] = field(default_factory=dict)
    edges: list[CFGEdge] = field(default_factory=list)
    entry_id: int = 0
    exit_id: int = 1
