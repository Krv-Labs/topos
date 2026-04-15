"""
Dependency Evaluation Section: Observation Space -> Subobject Classifier
------------------------------------------------------------------------

Maps dependency-graph metric observations into the evaluation lattice.

This is the coupling-dimension counterpart of ``structural.py``.  The
factorization is the same:

    DependencyGraph --(metrics)--> R^n --(section)--> Omega

The bins below encode interpretive commitments about when coupling or
instability levels are concerning.  Adjusting a bin boundary is a policy
decision -- the same coupling score can mean different things for a
microservice vs. a monolith.  These defaults target general-purpose
Python codebases.

Note: ``DependencyEvaluationSection`` inherits *only* the ``BinClassifier``
mixin (shared bin-walking machinery).  It intentionally does **not** inherit
``StructuralEvaluationSection``; calling ``classify_complexity`` or
``classify_entropy`` on this section would be a programming error.
"""

from __future__ import annotations

from math import inf
from typing import ClassVar

from topos.logic.lattice import EvaluationValue
from topos.logic.policies.base import BinClassifier, MetricDecision, ObservationBin


class DependencyEvaluationSection(BinClassifier):
    """
    Evaluation section for dependency-graph metrics.

    Provides ``classify_coupling`` and ``classify_instability`` using
    the same bin-walking machinery as the structural evaluation section.

    Only coupling and instability bins are defined here.  Calling any
    inherited method that requires ``complexity_bins`` or ``entropy_bins``
    (e.g. ``normalize_complexity``) will raise ``AttributeError`` — those
    bins belong to ``StructuralEvaluationSection``, not here.
    """

    coupling_bins: ClassVar[tuple[ObservationBin, ...]] = (
        ObservationBin(
            0,
            5,
            EvaluationValue.SOUND,
            "coupling within expected range",
        ),
        ObservationBin(
            5,
            12,
            EvaluationValue.STABLE,
            "coupling is elevated but manageable",
        ),
        ObservationBin(
            12,
            20,
            EvaluationValue.COMPLEX,
            "coupling is high and change-sensitive",
        ),
        ObservationBin(
            20,
            35,
            EvaluationValue.COUPLED,
            "coupling indicates entangled design",
        ),
        ObservationBin(
            35,
            inf,
            EvaluationValue.ENTANGLED,
            "coupling is pathologically high",
        ),
    )

    instability_bins: ClassVar[tuple[ObservationBin, ...]] = (
        ObservationBin(
            0.0,
            0.1,
            EvaluationValue.COMPLEX,
            "module is extremely stable -- hard to evolve",
        ),
        ObservationBin(
            0.1,
            0.3,
            EvaluationValue.STABLE,
            "module is fairly stable",
        ),
        ObservationBin(
            0.3,
            0.7,
            EvaluationValue.SOUND,
            "instability in balanced range",
        ),
        ObservationBin(
            0.7,
            0.9,
            EvaluationValue.STABLE,
            "module is fairly unstable",
        ),
        ObservationBin(
            0.9,
            inf,
            EvaluationValue.COMPLEX,
            "module is extremely unstable -- depends on everything",
        ),
    )

    @classmethod
    def classify_coupling(cls, raw_coupling: float) -> MetricDecision:
        """Map a total-coupling observation to Omega."""
        return cls._classify(raw_coupling, cls.coupling_bins)

    @classmethod
    def classify_instability(cls, raw_instability: float) -> MetricDecision:
        """Map an instability observation to Omega."""
        return cls._classify(raw_instability, cls.instability_bins)


dep_section = DependencyEvaluationSection()


def classify_coupling(raw_coupling: float) -> MetricDecision:
    return dep_section.classify_coupling(raw_coupling)


def classify_instability(raw_instability: float) -> MetricDecision:
    return dep_section.classify_instability(raw_instability)
