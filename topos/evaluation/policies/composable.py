"""
Φ_COMPOSABLE — Policy translator for the COMPOSABLE generator.
--------------------------------------------------------------

Maps ModuleDependencyGraph metric observations (Martin instability,
fan-in, fan-out) into a :class:`~topos.evaluation.policies.base.ScoredDecision`.
``achieved`` is the AND of independent raw thresholds on each metric;
``score`` is ``min(per-metric qualities)`` for reporting only.

Quality functions:
    instability_quality = piecewise flat-top tent over [low, high]:
                            instability in band → 1.0 (optimal range)
                            instability < low   → linear from 0.0 to 1.0
                            instability > high  → linear from 1.0 to 0.0

    fan_quality         = 1 - min(fan / cap, 1.0)
                          Linear fall from 1.0 to 0.0 at the cap.

The COMPOSABLE badge is achieved if all three metrics pass their
independent thresholds (AND logic). Gate comparisons and interpretation
prose live in :mod:`topos.evaluation.policies.gates`; thresholds in
:mod:`topos.evaluation.policies.calibration`.
"""

from __future__ import annotations

from topos.evaluation.policies.base import (
    Priority,
    ScoredDecision,
)
from topos.evaluation.policies.calibration import COMPOSABLE
from topos.evaluation.policies.gates import evaluate_gates


def score_coupling(
    instability: float | None = None,
    fan_in: float | None = None,
    fan_out: float | None = None,
    priority: Priority = Priority.SECURE,
    threshold: float | None = None,
    *,
    is_entrypoint_module: bool = False,
) -> ScoredDecision:
    """
    Φ_COMPOSABLE — score the COMPOSABLE generator using independent raw thresholds.

    Args:
        instability: Martin's instability metric, in [0.0, 1.0].
        fan_in:      Number of unique modules that depend on this module.
        fan_out:     Number of unique modules this module depends on.
        priority:    Retained for API compatibility; not read by this Φᵢ.
        threshold:   Retained for API compatibility; not read by this Φᵢ.
        is_entrypoint_module: When True, tolerate high instability for
            import/export-only entrypoint modules with zero fan-in.

    Returns:
        A ScoredDecision; ``achieved`` is the truth value of the COMPOSABLE
        generator for this program.
    """
    metrics = {
        key: value
        for key, value in {
            "mdg.instability": instability,
            "mdg.fan_in": fan_in,
            "mdg.fan_out": fan_out,
        }.items()
        if value is not None
    }
    results = evaluate_gates(
        metrics, pillar="composable", is_entrypoint_module=is_entrypoint_module
    )
    if not results:
        # If no metrics are provided, we vacuously satisfy COMPOSABLE.
        return ScoredDecision(score=1.0, achieved=True, interpretation={})

    # Score shaping (reporting only): quality curves stay local to Φ_COMPOSABLE.
    qualities = [_quality(r.spec.metric, r.value) for r in results]

    return ScoredDecision(
        # The combined score is the minimum of the individual qualities
        # (conservative AND).
        score=min(qualities),
        achieved=all(r.passed for r in results),
        interpretation={r.spec.metric: r.interpretation for r in results},
    )


# ---------------------------------------------------------------------------
# Private helpers
# ---------------------------------------------------------------------------


def _quality(metric: str, value: float) -> float:
    """Normalize one raw metric to a [0, 1] quality (never gates achieved)."""
    if metric == "mdg.instability":
        return _instability_tent(value)
    cap = (
        COMPOSABLE.max_fan_in_cap
        if metric == "mdg.fan_in"
        else COMPOSABLE.max_fan_out_cap
    )
    return 1.0 - min(value / cap, 1.0)


def _instability_tent(instability: float) -> float:
    """
    Flat-top tent function over [instability_low, instability_high].

    Returns 1.0 in the optimal range and falls linearly to 0.0 outside it.
    """
    low = COMPOSABLE.instability_low
    high = COMPOSABLE.instability_high
    if low <= instability <= high:
        return 1.0
    if instability < low:
        return instability / low
    # instability > high
    return max(0.0, (1.0 - instability) / (1.0 - high))
