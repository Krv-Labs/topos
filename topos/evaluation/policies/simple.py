"""
Φ_SIMPLE: Policy translator for the SIMPLE generator.
-----------------------------------------------------

Maps CFG and AST observations into a
:class:`~topos.evaluation.policies.base.ScoredDecision`.

    Φ_SIMPLE(metrics) → ScoredDecision
    achieved = (cyclomatic ≤ gate) ∧ (entropy in band) ∧ (max_func ≤ gate)
    score      = min(per-metric qualities)   # reporting only; does not gate achieved

This is the categorical formulation of the math spec §3 "Policy
Translation". Thresholds and normalization caps live in
:mod:`topos.evaluation.policies.calibration`.
"""

from __future__ import annotations

from topos.core.omega import Omega
from topos.evaluation.policies.base import (
    Priority,
    ScoredDecision,
)
from topos.evaluation.policies.calibration import SIMPLE


def score_simple(
    cyclomatic: float | None = None,
    entropy: float | None = None,
    max_function_complexity: float | None = None,
    priority: Priority = Priority.SECURE,
    threshold: float | None = None,
    *,
    is_entrypoint_module: bool = False,
) -> ScoredDecision:
    """
    Φ_SIMPLE — score the SIMPLE generator using independent raw thresholds.

    Args:
        cyclomatic: McCabe cyclomatic complexity (raw integer count).
        entropy:    Kolmogorov-proxy entropy from the source.
        max_function_complexity: Maximum McCabe complexity of any single function.
        priority:   Retained for API compatibility; not read by this Φᵢ.
        threshold:  Retained for API compatibility; not read by this Φᵢ.
        is_entrypoint_module: When True, tolerate low entropy for
            import/export-only entrypoint modules.

    Returns:
        A ScoredDecision; ``achieved`` is the truth value of the SIMPLE
        generator for this program.
    """
    achieved = True
    interp: dict[str, str] = {}
    qualities: list[float] = []

    # 1. CFG Cyclomatic Complexity
    if cyclomatic is not None:
        quality = 1.0 - min(cyclomatic / SIMPLE.max_cyclomatic_cap, 1.0)
        qualities.append(quality)
        if cyclomatic > SIMPLE.max_cyclomatic:
            achieved = False
        interp["cfg.cyclomatic"] = _cyclomatic_interpretation(cyclomatic, quality)

    # 2. AST Entropy
    if entropy is not None:
        quality = max(0.0, 1.0 - 2.0 * abs(entropy - SIMPLE.entropy_ideal))
        qualities.append(quality)
        if not (SIMPLE.min_entropy <= entropy <= SIMPLE.max_entropy) and not (
            is_entrypoint_module and entropy < SIMPLE.min_entropy
        ):
            achieved = False
        interp["ast.entropy"] = _entropy_interpretation(
            entropy, quality, is_entrypoint_module=is_entrypoint_module
        )

    # 3. AST Max Function Complexity
    if max_function_complexity is not None:
        quality = 1.0 - min(
            max_function_complexity / SIMPLE.max_function_complexity_cap, 1.0
        )
        qualities.append(quality)
        if max_function_complexity > SIMPLE.max_function_complexity:
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
    quality = max(0.0, 1.0 - 2.0 * abs(entropy - SIMPLE.entropy_ideal))
    return _entropy_interpretation(entropy, quality)


# ---------------------------------------------------------------------------
# Private interpretation helpers
# ---------------------------------------------------------------------------


def _cyclomatic_interpretation(raw: float, quality: float) -> str:
    if raw <= SIMPLE.max_cyclomatic:
        return (
            f"cyclomatic complexity ({raw:.0f}) within threshold "
            f"(<= {SIMPLE.max_cyclomatic})"
        )
    return (
        f"cyclomatic complexity ({raw:.0f}) exceeds threshold "
        f"(> {SIMPLE.max_cyclomatic})"
    )


def _max_func_interpretation(raw: float, quality: float) -> str:
    if raw <= SIMPLE.max_function_complexity:
        return (
            f"max function complexity ({raw:.0f}) within threshold "
            f"(<= {SIMPLE.max_function_complexity})"
        )
    return (
        f"max function complexity ({raw:.0f}) exceeds threshold "
        f"(> {SIMPLE.max_function_complexity})"
    )


def _entropy_interpretation(
    entropy: float, quality: float, *, is_entrypoint_module: bool = False
) -> str:
    if SIMPLE.min_entropy <= entropy <= SIMPLE.max_entropy:
        return (
            f"entropy ({entropy:.2f}) within structured range "
            f"[{SIMPLE.min_entropy}, {SIMPLE.max_entropy}]"
        )
    if entropy < SIMPLE.min_entropy:
        if is_entrypoint_module:
            return (
                f"entropy ({entropy:.2f}) is low, but tolerated for "
                "import/export-only entrypoint modules"
            )
        return f"entropy ({entropy:.2f}) is too low; code may be repetitive or trivial"
    return f"entropy ({entropy:.2f}) is too high; code may be unstructured"
