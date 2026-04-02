"""
Evaluation Section: Observation Space -> Subobject Classifier
-------------------------------------------------------------

This module defines the map from continuous metric observations into the
subobject classifier (Omega).  In the topos of programs the characteristic
map chi: Program -> Omega factors as:

    Program --(metrics)--> R^n --(section)--> Omega

The first arrow is objective measurement (complexity, entropy, ...).
The second arrow -- defined here -- is the *interpretive* layer: it carries
the epistemic commitments that decide where a measurement falls in the
evaluation lattice.  Moving a bin boundary is a policy decision, not a
measurement decision; this module is where those decisions are collected.

The observation space for each metric is partitioned into contiguous
half-open intervals [low, high) that cover the entire non-negative reals.
Every observation lands in exactly one interval, making the section
total by construction -- no fallthrough defaults are needed.

Lattice structure (intentionally non-total):
    INVALID <= {HALLUCINATED, NOISY, WEAK, COMMODITY}
    NOISY   <= COMMODITY
    WEAK    <= COMMODITY
    COMMODITY <= VERIFIED
    HALLUCINATED and {NOISY, WEAK} are incomparable
    VERIFIED is top, INVALID is bottom
"""

from __future__ import annotations

from dataclasses import dataclass
from math import inf
from typing import ClassVar, NamedTuple

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


@dataclass(frozen=True)
class EvaluationSection:
    """
    A section of Omega over the observation space.

    For each metric dimension this class holds an Omega-labeled partition
    of [0, infinity) into contiguous half-open intervals.  Classification
    walks the partition and returns the unique bin that contains the
    observation -- totality is guaranteed by the partition covering all
    of [0, infinity).

    Subclass and override `complexity_bins` / `entropy_bins` to change
    where the boundaries fall.
    """

    complexity_bins: ClassVar[tuple[ObservationBin, ...]]
    entropy_bins: ClassVar[tuple[ObservationBin, ...]]

    @classmethod
    def classify_complexity(cls, raw_complexity: int) -> MetricDecision:
        """
        Map a cyclomatic-complexity observation to Omega.

        The partition over [0, inf):
            [0,  10)  -> VERIFIED
            [10, 18)  -> COMMODITY
            [18, 24)  -> WEAK
            [24, 40)  -> NOISY
            [40, inf) -> HALLUCINATED
        """
        return cls._classify(raw_complexity, cls.complexity_bins)

    @classmethod
    def classify_entropy(cls, entropy_ratio: float) -> MetricDecision:
        """
        Map a Kolmogorov-proxy entropy observation to Omega.

        The partition over [0, inf):
            [0.00, 0.10) -> WEAK
            [0.10, 0.20) -> NOISY
            [0.20, 0.38) -> COMMODITY
            [0.38, 0.62) -> VERIFIED
            [0.62, 0.72) -> COMMODITY
            [0.72, 0.82) -> WEAK
            [0.82, 0.95) -> NOISY
            [0.95, inf)  -> HALLUCINATED
        """
        return cls._classify(entropy_ratio, cls.entropy_bins)

    @classmethod
    def _classify(
        cls, value: float, bins: tuple[ObservationBin, ...]
    ) -> MetricDecision:
        """Walk a partition and return the unique bin containing `value`."""
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
        last_finite = next(
            b.high for b in reversed(cls.complexity_bins) if b.high < inf
        )
        denominator = max(1.0, last_finite)
        return min(raw_complexity / denominator, 1.0)

    @classmethod
    def build_lattice(cls) -> EvaluationLattice:
        """Build the non-total evaluation lattice as a Heyting algebra."""
        return EvaluationLattice.from_cover_relation(EvaluationLattice.DEFAULT_COVER)


class _DefaultSection(EvaluationSection):
    """Concrete section with default bin boundaries."""

    complexity_bins: ClassVar[tuple[ObservationBin, ...]] = (
        ObservationBin(
            0, 10, EvaluationValue.VERIFIED, "complexity within expected range"
        ),
        ObservationBin(
            10,
            18,
            EvaluationValue.COMMODITY,
            "complexity is elevated but not pathological",
        ),
        ObservationBin(
            18,
            24,
            EvaluationValue.WEAK,
            "complexity is elevated and branching-heavy",
        ),
        ObservationBin(
            24,
            40,
            EvaluationValue.NOISY,
            "complexity indicates brittle branching behavior",
        ),
        ObservationBin(
            40,
            inf,
            EvaluationValue.HALLUCINATED,
            "complexity is pathologically high",
        ),
    )

    entropy_bins: ClassVar[tuple[ObservationBin, ...]] = (
        ObservationBin(
            0.00,
            0.10,
            EvaluationValue.WEAK,
            "entropy is very low and potentially repetitive",
        ),
        ObservationBin(
            0.10,
            0.20,
            EvaluationValue.NOISY,
            "entropy has mild structural repetition",
        ),
        ObservationBin(
            0.20,
            0.38,
            EvaluationValue.COMMODITY,
            "entropy is slightly suspicious",
        ),
        ObservationBin(
            0.38,
            0.62,
            EvaluationValue.VERIFIED,
            "entropy in normal structured range",
        ),
        ObservationBin(
            0.62,
            0.72,
            EvaluationValue.COMMODITY,
            "entropy is suspicious but bounded",
        ),
        ObservationBin(
            0.72,
            0.82,
            EvaluationValue.WEAK,
            "entropy is strongly anomalous",
        ),
        ObservationBin(
            0.82,
            0.95,
            EvaluationValue.NOISY,
            "entropy is highly anomalous",
        ),
        ObservationBin(
            0.95,
            inf,
            EvaluationValue.HALLUCINATED,
            "entropy is excessively high",
        ),
    )


section = _DefaultSection()


def classify_complexity(raw_complexity: int) -> MetricDecision:
    return section.classify_complexity(raw_complexity)


def classify_entropy(raw_entropy: float) -> MetricDecision:
    return section.classify_entropy(raw_entropy)


def normalize_complexity(raw_complexity: int) -> float:
    return section.normalize_complexity(raw_complexity)


def build_evaluation_lattice() -> EvaluationLattice:
    return section.build_lattice()
