"""
Structural test-coverage policy (outside Ω).
--------------------------------------------

The UAST probe emits a raw
:class:`~topos.functors.probes.uast.structural_test_coverage.DeclarationCoverageReport`;
this module threshold-classifies it into a :class:`CoverageDecision`
(mean recall, F2, uncovered declarations).  Independent of the three
quality generators in Ω.
"""

from __future__ import annotations

from dataclasses import dataclass, field

from topos.functors.probes.uast.structural_test_coverage import (
    DeclarationCoverageReport,
)


@dataclass(frozen=True)
class CoverageDecision:
    """Thresholded judgment over a raw declaration coverage report.

    Mirrors :class:`ScoredDecision` (score, achieved, interpretation) but
    carries coverage-specific fields (F2, uncovered declaration list).
    """

    score: float
    achieved: bool
    threshold: float
    coverage_rate: float
    f2_score: float
    uncovered_declarations: tuple[tuple[str, float], ...] = field(default_factory=tuple)
    interpretation: dict[str, str] = field(default_factory=dict)


def score_declaration_coverage(
    report: DeclarationCoverageReport,
    *,
    threshold: float = 0.5,
) -> CoverageDecision:
    """Threshold-classify a raw coverage report (mean recall vs ``threshold``)."""
    best_recall = tuple(report.best_declaration_recall)
    if best_recall:
        mean_declaration_coverage = sum(best_recall) / len(best_recall)
        coverage_rate = sum(1 for score in best_recall if score >= threshold) / len(
            best_recall
        )
    else:
        mean_declaration_coverage = 1.0
        coverage_rate = 1.0

    mean_test_precision = report.mean_test_precision
    numerator = 5.0 * mean_test_precision * mean_declaration_coverage
    denominator = 4.0 * mean_test_precision + mean_declaration_coverage
    f2_score = numerator / denominator if denominator > 0.0 else 0.0

    uncovered = tuple(
        (location, score)
        for location, score in zip(
            report.declaration_locations, best_recall, strict=False
        )
        if score < threshold
    )

    interpretation = {
        "declaration_coverage": _coverage_interpretation(
            mean_declaration_coverage, threshold
        ),
        "declaration_coverage_rate": (
            f"{coverage_rate:.0%} of declarations meet the {threshold:.2f} threshold"
        ),
        "declaration_f2_score": f"F2 score is {f2_score:.3f}",
    }

    return CoverageDecision(
        score=mean_declaration_coverage,
        achieved=mean_declaration_coverage >= threshold,
        threshold=threshold,
        coverage_rate=coverage_rate,
        f2_score=f2_score,
        uncovered_declarations=uncovered,
        interpretation=interpretation,
    )


def _coverage_interpretation(score: float, threshold: float) -> str:
    if score >= threshold + 0.25:
        return "coverage is strong"
    if score >= threshold:
        return "coverage meets the policy threshold"
    if score >= threshold * 0.5:
        return "coverage is partial"
    return "coverage is weak"
