"""
Φ_SIMPLE: Policy translator for the SIMPLE generator.
-----------------------------------------------------

Maps CFG and AST observations into a
:class:`~topos.evaluation.policies.base.ScoredDecision`.

    Φ_SIMPLE(metrics) → ScoredDecision
    achieved = (cyclomatic ≤ 15) ∧ (0.2 ≤ entropy ≤ 0.8) ∧ (max_func ≤ 10)
    score      = min(per-metric qualities)   # reporting only; does not gate achieved

This is the categorical formulation of the math spec §3 "Policy
Translation". All decisions (thresholds, combinations) are centralized here.
"""

from __future__ import annotations

from topos.core.omega import Omega
from topos.evaluation.policies.base import (
    Priority,
    ScoredDecision,
)

# Normalization caps (for [0, 1] mapping)
MAX_CYCLOMATIC_CAP: float = 40.0
ENTROPY_IDEAL: float = 0.5
MAX_FUNCTION_COMPLEXITY_CAP: float = 20.0

# Independent Raw Thresholds (Policy Decisions)
MAX_CYCLOMATIC_THRESHOLD: float = 15.0
MAX_FUNCTION_COMPLEXITY_THRESHOLD: float = 10.0
MIN_ENTROPY_THRESHOLD: float = 0.2
MAX_ENTROPY_THRESHOLD: float = 0.8


def score_simple(
    cyclomatic: float | None = None,
    entropy: float | None = None,
    max_function_complexity: float | None = None,
    priority: Priority = Priority.SECURE,
    threshold: float | None = None,
) -> ScoredDecision:
    """
    Φ_SIMPLE — score the SIMPLE generator using independent raw thresholds.

    Args:
        cyclomatic: McCabe cyclomatic complexity (raw integer count).
        entropy:    Kolmogorov-proxy entropy from the source.
        max_function_complexity: Maximum McCabe complexity of any single function.
        priority:   Retained for API compatibility; not read by this Φᵢ.
        threshold:  Retained for API compatibility; not read by this Φᵢ.

    Returns:
        A ScoredDecision; ``achieved`` is the truth value of the SIMPLE
        generator for this program.
    """
    achieved = True
    interp: dict[str, str] = {}
    qualities: list[float] = []

    # 1. CFG Cyclomatic Complexity
    if cyclomatic is not None:
        quality = 1.0 - min(cyclomatic / MAX_CYCLOMATIC_CAP, 1.0)
        qualities.append(quality)
        if cyclomatic > MAX_CYCLOMATIC_THRESHOLD:
            achieved = False
        interp["cfg.cyclomatic"] = _cyclomatic_interpretation(cyclomatic, quality)

    # 2. AST Entropy
    if entropy is not None:
        quality = max(0.0, 1.0 - 2.0 * abs(entropy - ENTROPY_IDEAL))
        qualities.append(quality)
        if not (MIN_ENTROPY_THRESHOLD <= entropy <= MAX_ENTROPY_THRESHOLD):
            achieved = False
        interp["ast.entropy"] = _entropy_interpretation(entropy, quality)

    # 3. AST Max Function Complexity
    if max_function_complexity is not None:
        quality = 1.0 - min(max_function_complexity / MAX_FUNCTION_COMPLEXITY_CAP, 1.0)
        qualities.append(quality)
        if max_function_complexity > MAX_FUNCTION_COMPLEXITY_THRESHOLD:
            achieved = False
        interp["ast.max_function_complexity"] = _max_func_interpretation(
            max_function_complexity, quality
        )

    if not qualities:
        # If no metrics are provided, we vacuously satisfy SIMPLE.
        return ScoredDecision(score=1.0, achieved=True, interpretation={})

    # The combined score is the minimum of the individual qualities (conservative AND).
    simple_score = min(qualities)

    return ScoredDecision(
        score=simple_score,
        achieved=achieved,
        interpretation=interp,
    )


def build_omega() -> Omega:
    """Build the subobject classifier Ω = H(G_qual) (8-element 3-cube)."""
    return Omega()


def describe_entropy_ratio(entropy: float) -> str:
    """Describe a raw AST entropy ratio using SIMPLE policy language."""
    quality = max(0.0, 1.0 - 2.0 * abs(entropy - ENTROPY_IDEAL))
    return _entropy_interpretation(entropy, quality)


# ---------------------------------------------------------------------------
# Private interpretation helpers
# ---------------------------------------------------------------------------


def _cyclomatic_interpretation(raw: float, quality: float) -> str:
    if raw <= MAX_CYCLOMATIC_THRESHOLD:
        return (
            f"cyclomatic complexity ({raw:.0f}) within threshold "
            f"(<= {MAX_CYCLOMATIC_THRESHOLD})"
        )
    return (
        f"cyclomatic complexity ({raw:.0f}) exceeds threshold "
        f"(> {MAX_CYCLOMATIC_THRESHOLD})"
    )


def _max_func_interpretation(raw: float, quality: float) -> str:
    if raw <= MAX_FUNCTION_COMPLEXITY_THRESHOLD:
        return (
            f"max function complexity ({raw:.0f}) within threshold "
            f"(<= {MAX_FUNCTION_COMPLEXITY_THRESHOLD})"
        )
    return (
        f"max function complexity ({raw:.0f}) exceeds threshold "
        f"(> {MAX_FUNCTION_COMPLEXITY_THRESHOLD})"
    )


def _entropy_interpretation(entropy: float, quality: float) -> str:
    if MIN_ENTROPY_THRESHOLD <= entropy <= MAX_ENTROPY_THRESHOLD:
        return (
            f"entropy ({entropy:.2f}) within structured range "
            f"[{MIN_ENTROPY_THRESHOLD}, {MAX_ENTROPY_THRESHOLD}]"
        )
    if entropy < MIN_ENTROPY_THRESHOLD:
        return f"entropy ({entropy:.2f}) is too low; code may be repetitive or trivial"
    return f"entropy ({entropy:.2f}) is too high; code may be unstructured"
