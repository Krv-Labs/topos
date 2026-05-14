"""
Policy translators Φᵢ: ℝ → ℋ — one per quality generator.

Each translator scores its dimension's metrics into ScoredDecision and
threshold-classifies the result against ℋ = H(G_qual).
"""

from topos.logic.policies.base import (
    WEIGHT_PROFILES,
    BinClassifier,
    MetricDecision,
    ObservationBin,
    Priority,
    ScoredDecision,
    WeightProfile,
)
from topos.logic.policies.coupling import score_coupling
from topos.logic.policies.secure import score_secure
from topos.logic.policies.simple import build_evaluation_lattice, score_simple

__all__ = [
    # Active scoring API
    "Priority",
    "WeightProfile",
    "WEIGHT_PROFILES",
    "ScoredDecision",
    "score_simple",
    "score_coupling",
    "score_secure",
    "build_evaluation_lattice",
    # Legacy (deprecated, kept for one release)
    "BinClassifier",
    "MetricDecision",
    "ObservationBin",
]
