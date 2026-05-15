"""
Φ_COMPOSABLE — Policy translator for the COMPOSABLE generator.
--------------------------------------------------------------

Maps ModuleDependencyGraph metric observations (Martin instability,
fan-in, fan-out) into a :class:`~topos.evaluation.policies.base.ScoredDecision`.
``achieved`` is the AND of independent raw thresholds on each metric;
``score`` is ``min(per-metric qualities)`` for reporting only.

Quality functions:
    instability_quality = piecewise flat-top tent over [0.3, 0.7]:
                            instability in [0.3, 0.7] → 1.0 (optimal range)
                            instability < 0.3           → linear from 0.0 to 1.0
                            instability > 0.7           → linear from 1.0 to 0.0

    fan_quality         = 1 - min(fan / MAX_FAN_CAP, 1.0)
                          Linear fall from 1.0 to 0.0 at the cap.

The COMPOSABLE badge is achieved if all three metrics pass their
independent thresholds (AND logic).
"""

from __future__ import annotations

from topos.evaluation.policies.base import (
    Priority,
    ScoredDecision,
)

# Normalization caps (for [0, 1] mapping)
MAX_FAN_IN_CAP: float = 40.0
MAX_FAN_OUT_CAP: float = 40.0

# Independent Raw Thresholds (Policy Decisions)
INSTABILITY_LOW: float = 0.3
INSTABILITY_HIGH: float = 0.7
MAX_FAN_IN_THRESHOLD: float = 15.0
MAX_FAN_OUT_THRESHOLD: float = 15.0


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
        if not (INSTABILITY_LOW <= instability <= INSTABILITY_HIGH):
            achieved = False
        interp["mdg.instability"] = _instability_interpretation(instability, quality)

    # 2. Fan-In
    if fan_in is not None:
        quality = 1.0 - min(fan_in / MAX_FAN_IN_CAP, 1.0)
        qualities.append(quality)
        if fan_in > MAX_FAN_IN_THRESHOLD:
            achieved = False
        interp["mdg.fan_in"] = _fan_interpretation("in", fan_in, quality)

    # 3. Fan-Out
    if fan_out is not None:
        quality = 1.0 - min(fan_out / MAX_FAN_OUT_CAP, 1.0)
        qualities.append(quality)
        if fan_out > MAX_FAN_OUT_THRESHOLD:
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
    Flat-top tent function over [INSTABILITY_LOW, INSTABILITY_HIGH].

    Returns 1.0 in the optimal range and falls linearly to 0.0 outside it.
    """
    if INSTABILITY_LOW <= instability <= INSTABILITY_HIGH:
        return 1.0
    if instability < INSTABILITY_LOW:
        return instability / INSTABILITY_LOW
    # instability > INSTABILITY_HIGH
    return max(0.0, (1.0 - instability) / (1.0 - INSTABILITY_HIGH))


def _instability_interpretation(instability: float, quality: float) -> str:
    if INSTABILITY_LOW <= instability <= INSTABILITY_HIGH:
        return (
            f"instability ({instability:.2f}) within balanced range "
            f"[{INSTABILITY_LOW}, {INSTABILITY_HIGH}]"
        )
    if instability < INSTABILITY_LOW:
        return f"instability ({instability:.2f}) is too low (module is too stable)"
    return (
        f"instability ({instability:.2f}) is too high "
        "(module depends on too many things)"
    )


def _fan_interpretation(direction: str, raw: float, quality: float) -> str:
    threshold = MAX_FAN_IN_THRESHOLD if direction == "in" else MAX_FAN_OUT_THRESHOLD
    if raw <= threshold:
        return f"fan-{direction} ({raw:.0f}) within threshold (<= {threshold})"
    return f"fan-{direction} ({raw:.0f}) exceeds threshold (> {threshold})"
