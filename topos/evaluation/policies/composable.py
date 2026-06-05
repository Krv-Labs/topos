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
independent thresholds (AND logic). Thresholds live in
:mod:`topos.evaluation.policies.calibration`.
"""

from __future__ import annotations

from topos.evaluation.policies.base import (
    Priority,
    ScoredDecision,
)
from topos.evaluation.policies.calibration import COMPOSABLE


def score_coupling(
    instability: float | None = None,
    fan_in: float | None = None,
    fan_out: float | None = None,
    priority: Priority = Priority.SECURE,
    threshold: float | None = None,
) -> ScoredDecision:
    """
    Φ_COMPOSABLE — score the COMPOSABLE generator using independent raw thresholds.

    Args:
        instability: Martin's instability metric, in [0.0, 1.0].
        fan_in:      Number of unique modules that depend on this module.
        fan_out:     Number of unique modules this module depends on.
        priority:    Retained for API compatibility; not read by this Φᵢ.
        threshold:   Retained for API compatibility; not read by this Φᵢ.

    Returns:
        A ScoredDecision; ``achieved`` is the truth value of the COMPOSABLE
        generator for this program.
    """
    achieved = True
    interp: dict[str, str] = {}
    qualities: list[float] = []

    # 1. Martin Instability
    if instability is not None:
        quality = _instability_tent(instability)
        qualities.append(quality)
        if not (COMPOSABLE.instability_low <= instability <= COMPOSABLE.instability_high):
            achieved = False
        interp["mdg.instability"] = _instability_interpretation(instability, quality)

    # 2. Fan-In
    if fan_in is not None:
        quality = 1.0 - min(fan_in / COMPOSABLE.max_fan_in_cap, 1.0)
        qualities.append(quality)
        if fan_in > COMPOSABLE.max_fan_in:
            achieved = False
        interp["mdg.fan_in"] = _fan_interpretation("in", fan_in, quality)

    # 3. Fan-Out
    if fan_out is not None:
        quality = 1.0 - min(fan_out / COMPOSABLE.max_fan_out_cap, 1.0)
        qualities.append(quality)
        if fan_out > COMPOSABLE.max_fan_out:
            achieved = False
        interp["mdg.fan_out"] = _fan_interpretation("out", fan_out, quality)

    if not qualities:
        # If no metrics are provided, we vacuously satisfy COMPOSABLE.
        return ScoredDecision(score=1.0, achieved=True, interpretation={})

    # The combined score is the minimum of the individual qualities (conservative AND).
    composable_score = min(qualities)

    return ScoredDecision(
        score=composable_score,
        achieved=achieved,
        interpretation=interp,
    )


# ---------------------------------------------------------------------------
# Private helpers
# ---------------------------------------------------------------------------


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


def _instability_interpretation(instability: float, quality: float) -> str:
    low = COMPOSABLE.instability_low
    high = COMPOSABLE.instability_high
    if low <= instability <= high:
        return (
            f"instability ({instability:.2f}) within balanced range "
            f"[{low}, {high}]"
        )
    if instability < low:
        return f"instability ({instability:.2f}) is too low (module is too stable)"
    return (
        f"instability ({instability:.2f}) is too high "
        "(module depends on too many things)"
    )


def _fan_interpretation(direction: str, raw: float, quality: float) -> str:
    gate = COMPOSABLE.max_fan_in if direction == "in" else COMPOSABLE.max_fan_out
    if raw <= gate:
        return f"fan-{direction} ({raw:.0f}) within threshold (<= {gate})"
    return f"fan-{direction} ({raw:.0f}) exceeds threshold (> {gate})"
