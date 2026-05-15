"""
Φ_SECURE: Policy translator for the SECURE generator.
-----------------------------------------------------

Maps CPG-based security observations into a
:class:`~topos.evaluation.policies.base.ScoredDecision`.
``achieved`` requires zero dangerous calls and zero taint flows;
``score`` is ``min(per-metric qualities)`` for reporting only.

Quality functions:
    danger_quality = exp(-dangerous_calls / DANGER_SCALE)
    taint_quality  = exp(-taint_flows / TAINT_SCALE)

The SECURE badge is achieved if and only if there are zero dangerous calls
and zero taint flows (strict security).
"""

from __future__ import annotations

from math import exp

from topos.evaluation.policies.base import (
    Priority,
    ScoredDecision,
)
from topos.evaluation.policies.base import (
    threshold as default_threshold,
)
from topos.evaluation.preferences import Generator

# Normalization scales (for [0, 1] mapping via exponential decay)
DANGER_SCALE: float = 3.0
TAINT_SCALE: float = 3.0

# Independent Raw Thresholds (Policy Decisions)
MAX_DANGEROUS_CALLS_THRESHOLD: float = 0.0
MAX_TAINT_FLOWS_THRESHOLD: float = 0.0


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
    achieved = True
    interp: dict[str, str] = {}
    qualities: list[float] = []

    # 1. Dangerous API Calls
    if dangerous_calls is not None:
        quality = exp(-max(dangerous_calls, 0.0) / DANGER_SCALE)
        qualities.append(quality)
        if dangerous_calls > MAX_DANGEROUS_CALLS_THRESHOLD:
            achieved = False
        interp["cpg.dangerous_calls"] = _danger_interpretation(dangerous_calls, quality)

    # 2. Taint Flows
    if taint_flows is not None:
        quality = exp(-max(taint_flows, 0.0) / TAINT_SCALE)
        qualities.append(quality)
        if taint_flows > MAX_TAINT_FLOWS_THRESHOLD:
            achieved = False
        interp["cpg.taint_flows"] = _taint_interpretation(taint_flows, quality)

    if not qualities:
        # If no metrics are provided, we vacuously satisfy SECURE.
        return ScoredDecision(score=1.0, achieved=True, interpretation={})

    # The combined score is the minimum of the individual qualities (conservative AND).
    secure_score = min(qualities)

    return ScoredDecision(
        score=secure_score,
        achieved=achieved,
        interpretation=interp,
    )


def _danger_interpretation(count: float, quality: float) -> str:
    if count <= MAX_DANGEROUS_CALLS_THRESHOLD:
        return f"no reachable dangerous-API calls ({count:.0f} <= {MAX_DANGEROUS_CALLS_THRESHOLD})"
    return f"{int(count)} dangerous-API call site(s) exceeds threshold ({MAX_DANGEROUS_CALLS_THRESHOLD})"


def _taint_interpretation(count: float, quality: float) -> str:
    if count <= MAX_TAINT_FLOWS_THRESHOLD:
        return f"no source→sink taint paths ({count:.0f} <= {MAX_TAINT_FLOWS_THRESHOLD})"
    return f"{int(count)} taint flow path(s) exceeds threshold ({MAX_TAINT_FLOWS_THRESHOLD})"
