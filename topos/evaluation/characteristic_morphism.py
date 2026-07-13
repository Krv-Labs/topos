"""Characteristic morphism — thin wrapper over the Rust engine."""

from __future__ import annotations

from collections.abc import Iterable

from topos.core.omega import EvaluationValue, Omega
from topos.topos_functors import CharacteristicMorphism, ClassificationResult

__all__ = ["CharacteristicMorphism", "ClassificationResult"]

_DIMENSION_GENERATOR: dict[str, EvaluationValue] = {
    "simple": EvaluationValue.SIMPLE,
    "composable": EvaluationValue.COMPOSABLE,
    "secure": EvaluationValue.SECURE,
}


def _combine_dimensions(
    self: CharacteristicMorphism,
    results: Iterable[ClassificationResult],
    thresholds: dict[str, float] | None = None,
) -> dict[str, EvaluationValue]:
    """
    Pointwise multi-file meet ⋀_f χ_S(f).

    A generator is satisfied across the codebase iff it is satisfied for
    every file (minimum score across files ≥ its calibrated threshold).
    Parse failures inject a zero score on the SIMPLE generator (since the
    program failed even to compile, no other generator is reachable).
    """
    del self
    if thresholds is None:
        from topos.evaluation.policies.base import THRESHOLDS, Generator

        thresholds = {
            "simple": THRESHOLDS[Generator.SIMPLE],
            "composable": THRESHOLDS[Generator.COMPOSABLE],
            "secure": THRESHOLDS[Generator.SECURE],
        }

    min_scores: dict[str, float] = {}
    for result in results:
        if not result.is_parseable:
            min_scores["simple"] = min(min_scores.get("simple", 1.0), 0.0)
        for dim, score in result.scores.items():
            if dim not in min_scores or score < min_scores[dim]:
                min_scores[dim] = score

    combined: dict[str, EvaluationValue] = {}
    for dim, score in min_scores.items():
        generator = _DIMENSION_GENERATOR.get(dim, EvaluationValue.SLOP)
        t = thresholds.get(dim, 0.6)
        combined[dim] = generator if score >= t else EvaluationValue.SLOP
    return combined


def _combine(self: CharacteristicMorphism, *values: EvaluationValue) -> EvaluationValue:
    """Combine multiple Ω values via meet (∧)."""
    del self
    return Omega().combine(*values)


CharacteristicMorphism.combine_dimensions = _combine_dimensions  # type: ignore[method-assign]
CharacteristicMorphism.combine = _combine  # type: ignore[method-assign]
