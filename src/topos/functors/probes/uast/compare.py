"""
UAST Comparison Module
----------------------
Cross-language structural comparison built on top of UAST `kind` values.

The existing `topos.metrics.distance` module is grammar-specific because
it consumes raw tree-sitter node types. This module is the language-agnostic
counterpart: every distance here operates on normalized UAST kinds, so two
implementations of the same algorithm in different languages can be compared
on a single axis.
"""

from __future__ import annotations

from dataclasses import dataclass

from topos.functors.profunctors.distance import DistanceResult, _compute_sequence_distance
from topos.functors.probes.uast.signature import (
    CONTROL_FLOW_KINDS,
    StructuralSummary,
    control_flow_profile,
    structural_summary,
    uast_dfs_kind_sequence,
    uast_kind_histogram,
)


@dataclass(frozen=True)
class UASTComparison:
    """Aggregate cross-language comparison between two UAST roots."""

    kind_distance: float
    edit_distance: DistanceResult
    control_flow_delta: dict[str, int]
    summary_delta: dict[str, int]
    source_summary: StructuralSummary
    target_summary: StructuralSummary

    @property
    def detects_difference(self) -> bool:
        """True if any signature reports a non-zero divergence."""
        if self.kind_distance > 0.0:
            return True
        if self.edit_distance.raw_distance > 0:
            return True
        if any(value != 0 for value in self.control_flow_delta.values()):
            return True
        return any(value != 0 for value in self.summary_delta.values())


def uast_kind_distance(
    source,
    target,
    *,
    include_unknown: bool = True,
) -> float:
    """
    L1 distance between normalized UAST kind histograms.

    Both histograms are normalized to probability distributions so the
    result lies in `[0, 1]` regardless of program size. A return value of
    `0.0` means both programs use the same mix of UAST kinds; `1.0` means
    they share no kinds at all.
    """
    a = uast_kind_histogram(source, include_unknown=include_unknown)
    b = uast_kind_histogram(target, include_unknown=include_unknown)

    total_a = sum(a.values()) or 1
    total_b = sum(b.values()) or 1

    kinds = set(a) | set(b)
    l1 = sum(abs(a.get(k, 0) / total_a - b.get(k, 0) / total_b) for k in kinds)
    # L1 distance between two probability distributions is bounded by 2;
    # halve to get a [0, 1] range matching the rest of the codebase.
    return l1 / 2.0


def uast_edit_distance(
    source,
    target,
    *,
    include_unknown: bool = True,
) -> DistanceResult:
    """
    Tree edit distance over UAST kind sequences (DFS pre-order).

    Reuses the Wagner-Fischer implementation from `topos.metrics.distance`
    so the operation accounting stays consistent with the tree-sitter
    variant.
    """
    source_kinds = uast_dfs_kind_sequence(source, include_unknown=include_unknown)
    target_kinds = uast_dfs_kind_sequence(target, include_unknown=include_unknown)

    distance, ops = _compute_sequence_distance(source_kinds, target_kinds)
    max_size = max(len(source_kinds), len(target_kinds), 1)
    normalized = min(distance / max_size, 1.0)

    return DistanceResult(
        raw_distance=distance,
        normalized_distance=normalized,
        operations=ops,
    )


def _control_flow_delta(source, target) -> dict[str, int]:
    src_profile = control_flow_profile(source)
    tgt_profile = control_flow_profile(target)
    return {kind: tgt_profile[kind] - src_profile[kind] for kind in CONTROL_FLOW_KINDS}


def _summary_delta(
    source_summary: StructuralSummary,
    target_summary: StructuralSummary,
) -> dict[str, int]:
    return {
        "node_count": target_summary.node_count - source_summary.node_count,
        "depth": target_summary.depth - source_summary.depth,
        "declaration_count": (
            target_summary.declaration_count - source_summary.declaration_count
        ),
        "expression_count": (
            target_summary.expression_count - source_summary.expression_count
        ),
        "statement_count": (
            target_summary.statement_count - source_summary.statement_count
        ),
    }


def compare_uast(
    source,
    target,
    *,
    include_unknown: bool = True,
) -> UASTComparison:
    """Run the full UAST comparison suite for a single pair of roots."""
    source_summary = structural_summary(source)
    target_summary = structural_summary(target)
    return UASTComparison(
        kind_distance=uast_kind_distance(
            source, target, include_unknown=include_unknown
        ),
        edit_distance=uast_edit_distance(
            source, target, include_unknown=include_unknown
        ),
        control_flow_delta=_control_flow_delta(source, target),
        summary_delta=_summary_delta(source_summary, target_summary),
        source_summary=source_summary,
        target_summary=target_summary,
    )
