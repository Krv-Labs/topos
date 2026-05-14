"""
Φ_SECURE: Policy translator for the SECURE generator.
-----------------------------------------------------

Maps CPG-based security observations into a normalized quality score in
[0, 1] and threshold-classifies it against the SECURE generator.

    Φ_SECURE : ℝ -> ℋ,  Φ_SECURE(metrics) ≥ threshold  ⇒  SECURE satisfied

The probes operate on the Code Property Graph (Yamaguchi et al.,
arxiv:1909.03496) and produce two principal metrics:

    cpg.dangerous_calls      — count of reachable dangerous-API call sites
    cpg.taint_flows          — count of source→sink data-flow paths

Quality functions
=================
    danger_quality = exp(-dangerous_calls / DANGER_SCALE)
        Exponentially decays from 1.0 (no dangerous calls) toward 0.0 as
        the count grows.

    taint_quality  = exp(-taint_flows / TAINT_SCALE)
        Same shape, applied to taint-flow paths.

The combined secure_score = w_t * taint_quality + (1-w_t) * danger_quality
where ``w_t`` is ``WeightProfile.w_taint`` for the active Priority.
"""

from __future__ import annotations

from math import exp

from topos.logic.policies.base import (
    WEIGHT_PROFILES,
    Priority,
    ScoredDecision,
)

# Normalization constants (policy decisions)
DANGER_SCALE: float = 3.0  # dangerous-call count that drops quality to 1/e
TAINT_SCALE: float = 3.0  # taint-flow count that drops quality to 1/e


def score_secure(
    dangerous_calls: float,
    taint_flows: float,
    priority: Priority = Priority.BALANCED,
    threshold: float = 0.6,
) -> ScoredDecision:
    """
    Φ_SECURE — score the SECURE generator from CPG observations.

    Args:
        dangerous_calls: Count of reachable dangerous-API call sites
                         (``cpg.dangerous_calls``).
        taint_flows:     Count of source→sink data-flow paths
                         (``cpg.taint_flows``).
        priority:        Weight profile controlling danger vs taint emphasis.
        threshold:       Minimum score to mark SECURE as satisfied.
    """
    danger_quality = exp(-max(dangerous_calls, 0.0) / DANGER_SCALE)
    taint_quality = exp(-max(taint_flows, 0.0) / TAINT_SCALE)

    w_t = WEIGHT_PROFILES[priority].w_taint
    secure_score = w_t * taint_quality + (1.0 - w_t) * danger_quality

    return ScoredDecision(
        score=secure_score,
        achieved=secure_score >= threshold,
        interpretation={
            "cpg.dangerous_calls": _danger_interpretation(
                dangerous_calls, danger_quality
            ),
            "cpg.taint_flows": _taint_interpretation(taint_flows, taint_quality),
        },
    )


def _danger_interpretation(count: float, quality: float) -> str:
    if count <= 0:
        return "no reachable dangerous-API calls"
    if quality >= 0.5:
        return f"{int(count)} dangerous-API call site(s); audit each"
    return f"{int(count)} dangerous-API call sites; pervasive risk"


def _taint_interpretation(count: float, quality: float) -> str:
    if count <= 0:
        return "no source→sink taint paths"
    if quality >= 0.5:
        return f"{int(count)} taint flow path(s); sanitize inputs"
    return f"{int(count)} taint flow paths; exploitability is high"
