"""
Φ_SIMPLE: Policy translator for the SIMPLE generator.
-----------------------------------------------------

Maps CFG-based observations (cyclomatic complexity, optionally entropy
during the migration window) into a normalized quality score in [0, 1]
and threshold-classifies it against the SIMPLE generator.

    Φ_SIMPLE : ℝ -> ℋ,  Φ_SIMPLE(metrics) ≥ threshold  ⇒  SIMPLE satisfied

This is the categorical formulation of the math spec §3 "Policy
Translation".  Probes on the CFG functor produce real numbers; this
module decides the truth value.

Quality functions
=================
    cyclomatic_quality = 1 - min(cyclomatic / MAX_CYCLOMATIC, 1.0)
        Linear fall from 1.0 (one straight-line path) to 0.0 at the cap.

    entropy_quality    = max(0, 1 - 2 * |entropy - ENTROPY_IDEAL|)
        Bell tent peaking at 0.5; included while ``ast.entropy`` is still a
        live probe.  Folded into the SIMPLE score per the math spec.

The combined simple_score = w_c * cyclomatic_quality + (1-w_c) * entropy_quality
where ``w_c`` comes from the active Priority's ``WeightProfile``.
"""

from __future__ import annotations

from topos.core.omega import Omega
from topos.evaluation.policies.base import (
    WEIGHT_PROFILES,
    Priority,
    ScoredDecision,
)
from topos.evaluation.policies.base import (
    threshold as default_threshold,
)
from topos.evaluation.preferences import Generator

# Normalization constants (policy decisions)
MAX_CYCLOMATIC: float = 40.0  # cyclomatic at which quality reaches 0.0
ENTROPY_IDEAL: float = 0.5  # entropy value with maximum quality score


def score_simple(
    cyclomatic: float,
    entropy: float | None = None,
    priority: Priority = Priority.SECURE,
    threshold: float | None = None,
) -> ScoredDecision:
    """
    Φ_SIMPLE — score the SIMPLE generator from CFG / AST observations.

    Args:
        cyclomatic: McCabe cyclomatic complexity (raw integer count).
                    Computed on the CFG: ``E - N + 2P``.
        entropy:    Optional Kolmogorov-proxy entropy from the source.  If
                    ``None``, only cyclomatic contributes to the score.
        priority:   Weight profile controlling cyclomatic vs entropy emphasis.
        threshold:  Minimum score to mark SIMPLE as satisfied.  When
                    ``None``, falls back to ``THRESHOLDS[Generator.SIMPLE]``
                    in :mod:`topos.evaluation.policies.base`.

    Returns:
        A ScoredDecision; ``achieved`` is the truth value of the SIMPLE
        generator for this program.
    """
    if threshold is None:
        threshold = default_threshold(Generator.SIMPLE)
    cyclomatic_quality = 1.0 - min(cyclomatic / MAX_CYCLOMATIC, 1.0)
    w_c = WEIGHT_PROFILES[priority].w_complexity

    interp: dict[str, str] = {
        "cfg.cyclomatic": _cyclomatic_interpretation(cyclomatic_quality),
    }

    if entropy is None:
        simple_score = cyclomatic_quality
    else:
        entropy_quality = max(0.0, 1.0 - 2.0 * abs(entropy - ENTROPY_IDEAL))
        simple_score = w_c * cyclomatic_quality + (1.0 - w_c) * entropy_quality
        interp["ast.entropy"] = _entropy_interpretation(entropy, entropy_quality)

    return ScoredDecision(
        score=simple_score,
        achieved=simple_score >= threshold,
        interpretation=interp,
    )


def build_omega() -> Omega:
    """Build the subobject classifier Ω = H(G_qual) (8-element 3-cube)."""
    return Omega()


# ---------------------------------------------------------------------------
# Private interpretation helpers
# ---------------------------------------------------------------------------


def _cyclomatic_interpretation(quality: float) -> str:
    if quality >= 0.75:
        return "cyclomatic complexity within expected range"
    if quality >= 0.5:
        return "cyclomatic complexity is elevated but manageable"
    if quality >= 0.25:
        return "cyclomatic complexity is high and branching-heavy"
    return "cyclomatic complexity is pathologically high"


def _entropy_interpretation(entropy: float, quality: float) -> str:
    if quality >= 0.75:
        return "entropy in normal structured range"
    if quality >= 0.5:
        if entropy < ENTROPY_IDEAL:
            return "entropy is slightly low; code may be repetitive"
        return "entropy is slightly high; code may be unstructured"
    if quality >= 0.25:
        if entropy < ENTROPY_IDEAL:
            return "entropy is strongly low; high structural repetition"
        return "entropy is strongly high; code is near-incompressible"
    if entropy < ENTROPY_IDEAL:
        return "entropy is extremely low; code is degenerate or trivial"
    return "entropy is extremely high; code has no exploitable structure"
