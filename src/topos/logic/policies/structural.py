"""
Evaluation Sections: Observation Space -> Subobject Classifier
--------------------------------------------------------------

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
    BROKEN <= {ENTANGLED, COUPLED, COMPLEX, STABLE}
    COUPLED <= STABLE
    COMPLEX <= STABLE
    STABLE  <= SOUND
    ENTANGLED and {COUPLED, COMPLEX} are incomparable
    SOUND is top, BROKEN is bottom
"""

from __future__ import annotations

from math import inf
from typing import ClassVar

from topos.logic.lattice import EvaluationLattice, EvaluationValue
from topos.logic.policies.base import BinClassifier, MetricDecision, ObservationBin


class StructuralEvaluationSection(BinClassifier):
    """
    Evaluation section for AST-based structural metrics.

    Maps cyclomatic complexity and Kolmogorov-proxy entropy observations
    into ``EvaluationValue``s using labelled half-open interval partitions.

    These bins encode the structural interpretation of raw measurements:
    adjusting a boundary is a policy decision about when complexity or
    entropy becomes structurally concerning.
    """

    complexity_bins: ClassVar[tuple[ObservationBin, ...]] = (
        ObservationBin(
            0, 10, EvaluationValue.SOUND, "complexity within expected range"
        ),
        ObservationBin(
            10,
            18,
            EvaluationValue.STABLE,
            "complexity is elevated but not pathological",
        ),
        ObservationBin(
            18,
            24,
            EvaluationValue.COMPLEX,
            "complexity is elevated and branching-heavy",
        ),
        ObservationBin(
            24,
            40,
            EvaluationValue.COUPLED,
            "complexity indicates brittle branching behavior",
        ),
        ObservationBin(
            40,
            inf,
            EvaluationValue.ENTANGLED,
            "complexity is pathologically high",
        ),
    )

    entropy_bins: ClassVar[tuple[ObservationBin, ...]] = (
        ObservationBin(
            0.00,
            0.10,
            EvaluationValue.COMPLEX,
            "entropy is very low and potentially repetitive",
        ),
        ObservationBin(
            0.10,
            0.20,
            EvaluationValue.COUPLED,
            "entropy has mild structural repetition",
        ),
        ObservationBin(
            0.20,
            0.38,
            EvaluationValue.STABLE,
            "entropy is slightly suspicious",
        ),
        ObservationBin(
            0.38,
            0.62,
            EvaluationValue.SOUND,
            "entropy in normal structured range",
        ),
        ObservationBin(
            0.62,
            0.72,
            EvaluationValue.STABLE,
            "entropy is suspicious but bounded",
        ),
        ObservationBin(
            0.72,
            0.82,
            EvaluationValue.COMPLEX,
            "entropy is strongly anomalous",
        ),
        ObservationBin(
            0.82,
            0.95,
            EvaluationValue.COUPLED,
            "entropy is highly anomalous",
        ),
        ObservationBin(
            0.95,
            inf,
            EvaluationValue.ENTANGLED,
            "entropy is excessively high",
        ),
    )

    @classmethod
    def classify_complexity(cls, raw_complexity: int) -> MetricDecision:
        """
        Map a cyclomatic-complexity observation to Omega.

        The partition over [0, inf):
            [0,  10)  -> SOUND
            [10, 18)  -> STABLE
            [18, 24)  -> COMPLEX
            [24, 40)  -> COUPLED
            [40, inf) -> ENTANGLED
        """
        return cls._classify(raw_complexity, cls.complexity_bins)

    @classmethod
    def classify_entropy(cls, entropy_ratio: float) -> MetricDecision:
        """
        Map a Kolmogorov-proxy entropy observation to Omega.

        The partition over [0, inf):
            [0.00, 0.10) -> COMPLEX
            [0.10, 0.20) -> COUPLED
            [0.20, 0.38) -> STABLE
            [0.38, 0.62) -> SOUND
            [0.62, 0.72) -> STABLE
            [0.72, 0.82) -> COMPLEX
            [0.82, 0.95) -> COUPLED
            [0.95, inf)  -> ENTANGLED
        """
        return cls._classify(entropy_ratio, cls.entropy_bins)


section = StructuralEvaluationSection()


def classify_complexity(raw_complexity: int) -> MetricDecision:
    return section.classify_complexity(raw_complexity)


def classify_entropy(raw_entropy: float) -> MetricDecision:
    return section.classify_entropy(raw_entropy)


def normalize_complexity(raw_complexity: int) -> float:
    return section.normalize_complexity(raw_complexity)


def build_evaluation_lattice() -> EvaluationLattice:
    return section.build_lattice()
