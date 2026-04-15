"""
Shared bin-walking machinery for evaluation sections.

``ObservationBin``, ``MetricDecision``, and ``BinClassifier`` are the
infrastructure used by every evaluation section.  Dimension-specific sections
(structural, coupling, ...) subclass ``BinClassifier`` and declare their own
bin partitions.
"""

from __future__ import annotations

from dataclasses import dataclass
from math import inf
from typing import NamedTuple

from topos.logic.lattice import EvaluationLattice, EvaluationValue


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
            raise AttributeError(
                f"{cls.__name__} does not define complexity_bins"
            )
        last_finite = next(
            b.high for b in reversed(complexity_bins) if b.high < inf
        )
        denominator = max(1.0, last_finite)
        return min(raw_complexity / denominator, 1.0)

    @classmethod
    def build_lattice(cls) -> EvaluationLattice:
        """Build the non-total evaluation lattice as a Heyting algebra."""
        return EvaluationLattice.from_cover_relation(EvaluationLattice.DEFAULT_COVER)
