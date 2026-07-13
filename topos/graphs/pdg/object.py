"""ProgramDependenceGraph — thin wrapper over the Rust engine."""

from topos.topos_functors import (
    DependenceEdge,
    DependenceKind,
    ProgramDependenceGraph,
)

__all__ = ["DependenceEdge", "DependenceKind", "ProgramDependenceGraph"]
