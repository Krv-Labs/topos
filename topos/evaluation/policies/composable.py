"""
Φ_COMPOSABLE — Policy translator for the COMPOSABLE generator.
--------------------------------------------------------------

Maps ModuleDependencyGraph metric observations (Martin instability,
fan-in, fan-out) plus Abstractness (mdg.abstractness, when available) into
a :class:`~topos.evaluation.policies.base.ScoredDecision`. ``achieved`` is
the AND of independent raw thresholds on each metric; ``score`` is
``min(per-metric qualities)`` for reporting only.

Quality functions:
    instability_quality = piecewise flat-top tent over [low, high]:
                            instability in band → 1.0 (optimal range)
                            instability < low   → linear from 0.0 to 1.0
                            instability > high  → linear from 1.0 to 0.0

    fan_quality         = 1 - min(fan / cap, 1.0)
                          Linear fall from 1.0 to 0.0 at the cap.

The COMPOSABLE badge is achieved if all present metrics pass their
independent thresholds (AND logic). Gate comparisons and interpretation
prose live in :mod:`topos.evaluation.policies.gates`; thresholds in
:mod:`topos.evaluation.policies.calibration`.

Distance from the Main Sequence (issue #124)
---------------------------------------------
Gating raw instability against a fixed band conflates two independent
concerns Robert Martin's own metric family keeps separate: how unstable a
module is (I), and how abstract it is (A). A concrete, unstable
orchestrator (main.rs: I≈1, A≈0) is *architecturally expected* — it sits
exactly on Martin's "Main Sequence" (A + I = 1) — but a raw-instability
band flags it anyway.

When ``abstractness`` is supplied, this scorer gates on
``mdg.main_sequence_distance = |A + I - 1|`` **instead of** raw
``mdg.instability`` (conditional replacement, not an additional stacked
gate — stacking would resurrect the false positive, since the old band
would still fail the orchestrator even though D says it's fine). Files/
languages without Abstractness data (``abstractness is None``) keep
gating on ``mdg.instability`` exactly as before — this is what makes the
change non-flag-day: it only changes behavior for files where Abstractness
is actually measured.

Files with zero *measured* coupling (``fan_in == 0`` and ``fan_out == 0``)
also keep gating on raw instability, even when ``abstractness`` is present.
``calculate_coupling`` returns ``instability = 0.5`` as a "no signal"
fallback for such files — optimal under the old flat-top instability band,
but combined with the common ``abstractness = 0.0`` case (no type
declarations) it lands ``main_sequence_distance`` exactly on
``main_sequence_distance_max``, passing the hard gate at the boundary while
scoring 0.0 on the distance quality curve. Excluding the no-signal case from
distance mode preserves the non-flag-day invariant above for coupling too.
"""

from __future__ import annotations

from topos.evaluation.policies.base import (
    Priority,
    ScoredDecision,
)
from topos.evaluation.policies.calibration import COMPOSABLE
from topos.evaluation.policies.gates import evaluate_gates, interpret_metric


def score_coupling(
    instability: float | None = None,
    fan_in: float | None = None,
    fan_out: float | None = None,
    abstractness: float | None = None,
    priority: Priority = Priority.SECURE,
    threshold: float | None = None,
    *,
    is_entrypoint_module: bool = False,
    is_stable_leaf_module: bool = False,
) -> ScoredDecision:
    """
    Φ_COMPOSABLE — score the COMPOSABLE generator using independent raw thresholds.

    Args:
        instability: Martin's instability metric, in [0.0, 1.0].
        fan_in:      Number of unique modules that depend on this module.
        fan_out:     Number of unique modules this module depends on.
        abstractness: Martin's abstractness metric, in [0.0, 1.0]. When
            provided (alongside ``instability``), gates on distance from
            the main sequence instead of raw instability — see module
            docstring.
        priority:    Retained for API compatibility; not read by this Φᵢ.
        threshold:   Retained for API compatibility; not read by this Φᵢ.
        is_entrypoint_module: When True, tolerate high instability for
            import/export-only entrypoint modules with zero fan-in. Applies
            only to the raw-instability gate (no abstractness available);
            in distance mode a concrete, unstable entrypoint already sits on
            the main sequence (D≈0), so no distance carve-out is needed — and
            an abstract, unstable entrypoint (D≈1) is Martin's "Zone of
            Uselessness", which has no accepted exception.
        is_stable_leaf_module: When True, tolerate maximal main-sequence
            distance for frozen, declarations-only leaf modules (Martin's
            "Zone of Pain" exception).

    Returns:
        A ScoredDecision; ``achieved`` is the truth value of the COMPOSABLE
        generator for this program.
    """
    has_coupling_signal = not (fan_in == 0.0 and fan_out == 0.0)
    use_distance = (
        instability is not None and abstractness is not None and has_coupling_signal
    )
    metrics = {
        key: value
        for key, value in {
            "mdg.instability": None if use_distance else instability,
            "mdg.abstractness": abstractness if use_distance else None,
            "mdg.main_sequence_distance": (
                abs(abstractness + instability - 1.0) if use_distance else None
            ),
            "mdg.fan_in": fan_in,
            "mdg.fan_out": fan_out,
        }.items()
        if value is not None
    }
    results = evaluate_gates(
        metrics,
        pillar="composable",
        is_entrypoint_module=is_entrypoint_module,
        is_stable_leaf_module=is_stable_leaf_module,
        instability=instability,
    )
    if not results:
        # If no metrics are provided, we vacuously satisfy COMPOSABLE.
        return ScoredDecision(score=1.0, achieved=True, interpretation={})

    # Score shaping (reporting only): quality curves stay local to Φ_COMPOSABLE.
    qualities = [_quality(r.spec.metric, r.value) for r in results]

    interpretation = {r.spec.metric: r.interpretation for r in results}
    if use_distance:
        # `mdg.instability` is deliberately not gated when distance is
        # active, but users should still see why a high/low instability
        # reading isn't itself a failure — surface it as an informational
        # (non-gating) line alongside the distance verdict.
        interpretation["mdg.instability"] = interpret_metric(
            "mdg.instability", instability
        )

    return ScoredDecision(
        # The combined score is the minimum of the individual qualities
        # (conservative AND).
        score=min(qualities),
        achieved=all(r.passed for r in results),
        interpretation=interpretation,
    )


# ---------------------------------------------------------------------------
# Private helpers
# ---------------------------------------------------------------------------


def _quality(metric: str, value: float) -> float:
    """Normalize one raw metric to a [0, 1] quality (never gates achieved)."""
    if metric == "mdg.instability":
        return _instability_tent(value)
    if metric == "mdg.main_sequence_distance":
        return _distance_quality(value)
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


def _distance_quality(distance: float) -> float:
    """Linear fall from 1.0 (on the main sequence) to 0.0 at the cap."""
    return 1.0 - min(distance / COMPOSABLE.main_sequence_distance_max, 1.0)
