"""
CFG Cycle-Basis Probe
-----------------------
Cyclomatic complexity (``cfg.cyclomatic``, E - N + 2P) is a summary
statistic — it tells you *how many* independent cycles a function's control
flow has, but not *which* cycles, or where they live in the source.

This probe extracts a fundamental cycle basis (spanning tree + back-edge
closure, see :mod:`ph` in the Rust crate) and maps each cycle generator back
to the source line range it covers, so a refactoring tool can point directly
at the loop body responsible for a complexity hotspot.

Purely advisory — never folded into ``cfg.*`` metrics or the SIMPLE score;
feeds ``topos refactor cycles`` (issue #83).
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import TYPE_CHECKING

from topos.functors.probes.cfg.complexity import _get_rust_cfg

if TYPE_CHECKING:
    from topos.graphs.cfg.object import ControlFlowGraph


@dataclass
class SourceCycle:
    """One cycle generator, mapped to the source it covers.

    Attributes:
        block_ids: The basic blocks (in walk order, closing duplicate
            removed) that make up this cycle.
        start_line: Earliest source line covered by any block in the cycle.
        end_line: Latest source line covered by any block in the cycle.
        file: Source file path, when available.
    """

    block_ids: list[int]
    start_line: int | None = None
    end_line: int | None = None
    file: str | None = None


@dataclass
class CfgHomologyResult:
    """Cycle basis for a CFG.

    Attributes:
        betti_1: Rank of the cycle space — equals ``cyclomatic - 1`` for the
            single-connected-component CFGs this builder always produces.
        cycles: One :class:`SourceCycle` per fundamental cycle.
    """

    betti_1: int
    cycles: list[SourceCycle] = field(default_factory=list)


def calculate_cycle_basis(cfg: ControlFlowGraph) -> CfgHomologyResult:
    """
    Extract a fundamental cycle basis and map each cycle to its source range.

    Reuses :func:`topos.functors.probes.cfg.complexity._get_rust_cfg`'s
    cached Python->Rust conversion rather than rebuilding it.
    """
    rust_cfg = _get_rust_cfg(cfg)
    rust_result = rust_cfg.cycle_basis()

    cycles = []
    for cycle in rust_result.cycles:
        # The Rust walk is a closed loop (first block id == last); dedupe
        # while preserving order for a cleaner block list to report.
        block_ids = list(dict.fromkeys(cycle.blocks))

        start_line: int | None = None
        end_line: int | None = None
        file: str | None = None
        for block_id in block_ids:
            block = cfg.blocks.get(block_id)
            if block is None:
                continue
            for stmt in block.statements:
                span = stmt.span
                if file is None:
                    file = span.file
                if start_line is None or span.start_line < start_line:
                    start_line = span.start_line
                if end_line is None or span.end_line > end_line:
                    end_line = span.end_line

        cycles.append(
            SourceCycle(
                block_ids=block_ids,
                start_line=start_line,
                end_line=end_line,
                file=file,
            )
        )

    return CfgHomologyResult(betti_1=rust_result.betti_1, cycles=cycles)
