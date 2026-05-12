"""
UAST Signature Module
---------------------
Cheap, language-agnostic structural fingerprints of a UAST root.

These signatures are designed to summarize a program's shape across
languages so that two implementations of the same algorithm can be
compared without depending on language-specific node names.
"""

from __future__ import annotations

from collections import Counter
from dataclasses import dataclass
from typing import Iterable

CONTROL_FLOW_KINDS: frozenset[str] = frozenset(
    {
        "IfStmt",
        "ForStmt",
        "WhileStmt",
        "MatchStmt",
        "TryStmt",
        "ReturnStmt",
        "BreakStmt",
        "ContinueStmt",
        "ThrowStmt",
        "CallExpr",
    }
)


@dataclass(frozen=True)
class StructuralSummary:
    """Aggregate, language-agnostic stats about a UAST."""

    node_count: int
    depth: int
    declaration_count: int
    expression_count: int
    statement_count: int


def _walk(root) -> Iterable[object]:
    yield root
    for child in getattr(root, "children", []):
        yield from _walk(child)


def uast_kind_histogram(root, *, include_unknown: bool = True) -> dict[str, int]:
    """
    Count UAST `kind` occurrences across the whole tree.

    Args:
        root: A UAST root node (duck-typed: must expose `kind` and `children`).
        include_unknown: When False, drops the catch-all `Unknown` bucket so
            grammar-coverage gaps don't dominate the histogram for languages
            with thinner UAST mapping coverage.
    """
    counts: Counter[str] = Counter()
    for node in _walk(root):
        kind = getattr(node, "kind", "Unknown")
        if not include_unknown and kind == "Unknown":
            continue
        counts[kind] += 1
    return dict(counts)


def control_flow_profile(root) -> dict[str, int]:
    """Count control-flow-relevant UAST kinds (loops, branches, calls, returns)."""
    profile = {kind: 0 for kind in CONTROL_FLOW_KINDS}
    for node in _walk(root):
        kind = getattr(node, "kind", "")
        if kind in profile:
            profile[kind] += 1
    return profile


def structural_summary(root) -> StructuralSummary:
    """Single-pass aggregate stats about the UAST."""
    node_count = 0
    declaration_count = 0
    expression_count = 0
    statement_count = 0

    def walk(node, depth: int) -> int:
        nonlocal node_count, declaration_count, expression_count, statement_count
        node_count += 1
        kind = getattr(node, "kind", "")
        if kind.endswith("Decl"):
            declaration_count += 1
        elif kind.endswith("Expr"):
            expression_count += 1
        elif kind.endswith("Stmt"):
            statement_count += 1
        max_depth = depth
        for child in getattr(node, "children", []):
            max_depth = max(max_depth, walk(child, depth + 1))
        return max_depth

    depth = walk(root, 0)
    return StructuralSummary(
        node_count=node_count,
        depth=depth,
        declaration_count=declaration_count,
        expression_count=expression_count,
        statement_count=statement_count,
    )
