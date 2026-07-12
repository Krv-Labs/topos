"""
Φ_SECURE: Policy translator for the SECURE generator.
-----------------------------------------------------

Maps CPG-based security observations into a
:class:`~topos.evaluation.policies.base.ScoredDecision`.
``achieved`` requires zero dangerous calls and zero taint flows;
``score`` is ``min(per-metric qualities)`` for reporting only.

Quality functions:
    danger_quality = exp(-dangerous_calls / danger_scale)
    taint_quality  = exp(-taint_flows / taint_scale)

The SECURE badge is achieved if and only if there are zero dangerous calls
and zero taint flows (strict security). Gate comparisons and interpretation
prose live in :mod:`topos.evaluation.policies.gates`; thresholds in
:mod:`topos.evaluation.policies.calibration`.
"""

from __future__ import annotations

from math import exp

from topos.evaluation.policies.base import (
    Priority,
    ScoredDecision,
)
from topos.evaluation.policies.calibration import SECURE
from topos.evaluation.policies.gates import evaluate_gates


def score_secure(
    dangerous_calls: float | None = None,
    taint_flows: float | None = None,
    priority: Priority = Priority.SECURE,
    threshold: float | None = None,
) -> ScoredDecision:
    """
    Φ_SECURE — score the SECURE generator from CPG observations.

    Args:
        dangerous_calls: Count of reachable dangerous-API call sites.
        taint_flows:     Count of source→sink data-flow paths.
        priority:        Retained for API compatibility; not read by this Φᵢ.
        threshold:       Retained for API compatibility; not read by this Φᵢ.

    Returns:
        A ScoredDecision; ``achieved`` is the truth value of the SECURE
        generator for this program.
    """
    metrics = {
        key: value
        for key, value in {
            "cpg.dangerous_calls": dangerous_calls,
            "cpg.taint_flows": taint_flows,
        }.items()
        if value is not None
    }
    results = evaluate_gates(metrics, pillar="secure")
    if not results:
        # If no metrics are provided, we vacuously satisfy SECURE.
        return ScoredDecision(score=1.0, achieved=True, interpretation={})

    # Score shaping (reporting only): exponential decay stays local to Φ_SECURE.
    scale = {
        "cpg.dangerous_calls": SECURE.danger_scale,
        "cpg.taint_flows": SECURE.taint_scale,
    }
    qualities = [exp(-max(r.value, 0.0) / scale[r.spec.metric]) for r in results]

    return ScoredDecision(
        # The combined score is the minimum of the individual qualities
        # (conservative AND).
        score=min(qualities),
        achieved=all(r.passed for r in results),
        interpretation={r.spec.metric: r.interpretation for r in results},
    )
