"""
Policy translators Φᵢ : ℝ → Ω — one per quality generator.

Each translator scores its dimension's metrics into a
:class:`ScoredDecision` and threshold-classifies the result against
Ω = H(G_qual).
"""

from topos.evaluation.policies.base import (
    WEIGHT_PROFILES,
    Priority,
    ScoredDecision,
    WeightProfile,
)
from topos.evaluation.policies.coupling import score_coupling
from topos.evaluation.policies.secure import score_secure
from topos.evaluation.policies.simple import build_omega, score_simple

__all__ = [
    "Priority",
    "WeightProfile",
    "WEIGHT_PROFILES",
    "ScoredDecision",
    "score_simple",
    "score_coupling",
    "score_secure",
    "build_omega",
]
