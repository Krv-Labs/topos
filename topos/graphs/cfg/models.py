"""
CFG Models
----------

Data structures for an intra-procedural Control Flow Graph built on top of
the language-independent UAST.

A CFG consists of *basic blocks* (maximal straight-line UAST-statement
sequences with single entry and single exit) connected by typed control-flow
edges:

    UNCONDITIONAL    — fall-through into the next block
    TRUE / FALSE     — conditional branches out of an IfStmt or loop test
    LOOP_BACK        — back-edge from end-of-body to loop header
    BREAK            — exit from a loop / switch
    CONTINUE         — back-edge to the loop test
    RETURN           — early return to the procedure exit block
    EXCEPTION        — try/catch fall-through
    SWITCH_CASE      — case-arm selection

The graph always has a unique *entry* block (synthetic) and a unique *exit*
block (synthetic).  This invariant is required for McCabe cyclomatic
complexity to evaluate as ``E - N + 2P`` with ``P = 1``.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from enum import StrEnum

from topos.graphs.uast.models import UASTNode


class EdgeKind(StrEnum):
    """Typed control-flow edge labels."""

    UNCONDITIONAL = "unconditional"
    TRUE = "true"
    FALSE = "false"
    LOOP_BACK = "loop_back"
    BREAK = "break"
    CONTINUE = "continue"
    RETURN = "return"
    EXCEPTION = "exception"
    SWITCH_CASE = "switch_case"


@dataclass
class BasicBlock:
    """
    A maximal straight-line sequence of UAST statements.

    Attributes:
        id:         Unique integer id within the owning CFG.
        statements: The UAST nodes executed in order on entry to this block.
                    Empty for the synthetic entry/exit blocks.
        label:      Human-readable label ("entry", "exit", "if_then", …).
    """

    id: int
    statements: list[UASTNode] = field(default_factory=list)
    label: str = ""

    def __repr__(self) -> str:  # pragma: no cover - cosmetic
        return f"BB#{self.id}({self.label or len(self.statements)})"


@dataclass(frozen=True)
class CFGEdge:
    """A typed edge between two basic blocks."""

    source: int
    target: int
    kind: EdgeKind
