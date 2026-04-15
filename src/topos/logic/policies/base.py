"""
Shared scoring infrastructure for evaluation sections.

This module defines two layers:

1. **New scoring layer** (Priority, WeightProfile, ScoredDecision):
   The active production API.  Scorers produce a continuous normalized score
   in [0, 1] and compare it against a threshold to determine whether a
   lattice target (COMPOSABLE or SELF_CONTAINED) is achieved.

2. **Legacy bin-walking layer** (ObservationBin, MetricDecision, BinClassifier):
   Kept for reference; no longer used in production after the diamond-lattice
   redesign.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from enum import StrEnum
from math import inf
from typing import NamedTuple

from topos.logic.lattice import EvaluationLattice, EvaluationValue

# ---------------------------------------------------------------------------
# Scoring layer (active)
# ---------------------------------------------------------------------------


class Priority(StrEnum):
    """
    Optimization priority that shifts metric weights within each dimension.

    Agents select a priority to express which quality axis matters most for
    the current task.  The priority controls internal weight profiles but does
    not change the lattice structure — COMPOSABLE and SELF_CONTAINED remain
    independent targets regardless of priority.

    Values:
        BALANCED:       Equal weight on all metrics (default).
        COMPOSABLE:     Upweights coupling metrics; optimizes for inter-module
                        composition quality.
        SELF_CONTAINED: Upweights structural metrics; optimizes for internal
                        complexity and entropy.
    """

    BALANCED = "balanced"
    COMPOSABLE = "composable"
    SELF_CONTAINED = "self_contained"


@dataclass(frozen=True)
class WeightProfile:
    """
    Per-dimension metric weights for a given Priority.

    Attributes:
        w_complexity: Weight on complexity_quality within the structural score
                      (vs entropy_quality, which gets 1 - w_complexity).
        w_coupling:   Weight on coupling_quality within the coupling score
                      (vs instability_quality, which gets 1 - w_coupling).
    """

    w_complexity: float
    w_coupling: float


WEIGHT_PROFILES: dict[Priority, WeightProfile] = {
    Priority.BALANCED: WeightProfile(w_complexity=0.5, w_coupling=0.5),
    Priority.COMPOSABLE: WeightProfile(w_complexity=0.3, w_coupling=0.7),
    Priority.SELF_CONTAINED: WeightProfile(w_complexity=0.7, w_coupling=0.3),
}


@dataclass(frozen=True)
class ScoredDecision:
    """
    Result of scoring a quality dimension with a continuous normalized score.

    Attributes:
        score:          Weighted quality score in [0.0, 1.0].  Higher is better.
        achieved:       True when score >= threshold (lattice target is met).
        interpretation: Per-metric human-readable strings keyed by metric name.
    """

    score: float
    achieved: bool
    interpretation: dict[str, str] = field(default_factory=dict)


class ObservationBin(NamedTuple):
    """A half-open interval [low, high) labeled with an evaluation value."""

    low: float
    high: float
    evaluation: EvaluationValue
    interpretation: str


@dataclass(frozen=True)
class MetricDecision:
    """Result of mapping a raw metric score through the evaluation section."""

    raw_score: float
    evaluation: EvaluationValue
    interpretation: str


class BinClassifier:
    """
    Shared bin-walking machinery for evaluation sections.

    Subclass this to build metric-specific classifiers.  Only the ``_classify``
    method is provided here -- no bin definitions, no metric-specific methods.
    Each subclass declares its own ``ClassVar`` bin tuples and exposes its own
    named classify methods.
    """

    @classmethod
    def _classify(
        cls, value: float, bins: tuple[ObservationBin, ...]
    ) -> MetricDecision:
        """Walk a partition and return the unique bin containing ``value``."""
        for b in bins:
            if b.low <= value < b.high:
                return MetricDecision(
                    raw_score=float(value),
                    evaluation=b.evaluation,
                    interpretation=b.interpretation,
                )
        raise ValueError(
            f"Observation {value} is outside the partition domain "
            f"[{bins[0].low}, {bins[-1].high})"
        )

    @classmethod
    def normalize_complexity(cls, raw_complexity: int) -> float:
        """Normalize complexity into [0, 1] for reporting."""
        # Find the last finite boundary across any complexity bins on the subclass.
        complexity_bins: tuple[ObservationBin, ...] | None = getattr(
            cls, "complexity_bins", None
        )
        if complexity_bins is None:
            raise AttributeError(f"{cls.__name__} does not define complexity_bins")
        last_finite = next(b.high for b in reversed(complexity_bins) if b.high < inf)
        denominator = max(1.0, last_finite)
        return min(raw_complexity / denominator, 1.0)

    @classmethod
    def build_lattice(cls) -> EvaluationLattice:
        """Build the non-total evaluation lattice as a Heyting algebra."""
        return EvaluationLattice.from_cover_relation(EvaluationLattice.DEFAULT_COVER)
