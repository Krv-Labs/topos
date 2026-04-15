from topos.logic.policies.base import (
    Priority,
    WeightProfile,
    WEIGHT_PROFILES,
    ScoredDecision,
    # Legacy (kept for compatibility during transition)
    BinClassifier,
    MetricDecision,
    ObservationBin,
)
from topos.logic.policies.coupling import score_coupling
from topos.logic.policies.structural import build_evaluation_lattice, score_structural

__all__ = [
    # Active scoring API
    "Priority",
    "WeightProfile",
    "WEIGHT_PROFILES",
    "ScoredDecision",
    "score_structural",
    "score_coupling",
    "build_evaluation_lattice",
    # Legacy (deprecated)
    "BinClassifier",
    "MetricDecision",
    "ObservationBin",
]
