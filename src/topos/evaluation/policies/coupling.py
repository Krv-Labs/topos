"""
Φ_COMPOSABLE — Policy translator for the COMPOSABLE generator.
--------------------------------------------------------------

Maps ModuleDependencyGraph metric observations (Martin coupling count,
instability ratio) into a continuous quality score that determines
whether the COMPOSABLE generator of Ω is satisfied.

The factorization is::

    ModuleDependencyGraph --(probes)--> ℝ² --(Φ_COMPOSABLE)--> [0, 1] --(threshold)--> Ω

``score_coupling`` is the interpretive layer for the COMPOSABLE
generator's metrics.  Normalization bounds and threshold are policy
decisions; this module is where they live.

Quality functions:
    coupling_quality    = 1 - min(coupling / MAX_COUPLING, 1.0)
                          Linear fall from 1.0 at coupling=0 to 0.0 at MAX.

    instability_quality = piecewise flat-top tent over [0.3, 0.7]:
                            instability in [0.3, 0.7] → 1.0 (optimal range)
                            instability < 0.3           → linear from 0.0 to 1.0
                            instability > 0.7           → linear from 1.0 to 0.0
                          Penalizes both extremely stable (hard to evolve) and
                          extremely unstable (depends on everything) modules.

The weighted coupling score = w_k * coupling_quality + (1-w_k) * instability_quality
where w_k comes from the Priority's WeightProfile.
"""

from __future__ import annotations

from topos.evaluation.policies.base import (
    WEIGHT_PROFILES,
    Priority,
    ScoredDecision,
)

# Normalization constants (policy decisions)
MAX_COUPLING: float = 35.0  # coupling count at which quality reaches 0.0
INSTABILITY_LOW: float = 0.3  # lower bound of optimal instability range
INSTABILITY_HIGH: float = 0.7  # upper bound of optimal instability range


def score_coupling(
    coupling: float,
    instability: float,
    priority: Priority,
    threshold: float = 0.6,
) -> ScoredDecision:
    """
    Score the coupling quality of a module.

    Args:
        coupling:    Total coupling count (fan-in + fan-out or similar).
        instability: Martin's instability metric, in [0.0, 1.0].
        priority:    Weight profile controlling coupling vs instability emphasis.
        threshold:   Minimum score to achieve the COMPOSABLE lattice target.

    Returns:
        A ScoredDecision with a [0, 1] quality score and per-metric
        interpretation strings.
    """
    coupling_quality = 1.0 - min(coupling / MAX_COUPLING, 1.0)
    instability_quality = _instability_tent(instability)

    w_k = WEIGHT_PROFILES[priority].w_coupling
    coupling_score = w_k * coupling_quality + (1.0 - w_k) * instability_quality

    return ScoredDecision(
        score=coupling_score,
        achieved=coupling_score >= threshold,
        interpretation={
            "mdg.coupling": _coupling_interpretation(coupling, coupling_quality),
            "mdg.instability": _instability_interpretation(
                instability, instability_quality
            ),
        },
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


def _coupling_interpretation(coupling: float, quality: float) -> str:
    if quality >= 0.75:
        return "coupling within expected range"
    if quality >= 0.5:
        return "coupling is elevated but manageable"
    if quality >= 0.25:
        return "coupling is high and change-sensitive"
    return "coupling is pathologically high; module is brittle"


def _instability_interpretation(instability: float, quality: float) -> str:
    if quality >= 0.75:
        return "instability in balanced range"
    if instability < INSTABILITY_LOW:
        if quality >= 0.5:
            return "module is fairly stable"
        return "module is extremely stable — hard to evolve"
    # instability > INSTABILITY_HIGH
    if quality >= 0.5:
        return "module is fairly unstable"
    return "module is extremely unstable — depends on everything"
