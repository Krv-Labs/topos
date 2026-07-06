"""
Φ_SIMPLE: Policy translator for the SIMPLE generator.
-----------------------------------------------------

Maps CFG and AST observations into a
:class:`~topos.evaluation.policies.base.ScoredDecision`.

    Φ_SIMPLE(metrics) → ScoredDecision
    achieved = (cyclomatic ≤ gate) ∧ (entropy in band) ∧ (max_func ≤ gate)
    score      = min(per-metric qualities)   # reporting only; does not gate achieved

This is the categorical formulation of the math spec §3 "Policy
Translation". Gate comparisons and interpretation prose live in
:mod:`topos.evaluation.policies.gates`; thresholds and normalization caps in
:mod:`topos.evaluation.policies.calibration`. Only the score-shaping quality
curves remain local.
"""

from __future__ import annotations

from topos.core.omega import Omega
from topos.evaluation.policies.base import (
    Priority,
    ScoredDecision,
)
from topos.evaluation.policies.calibration import SIMPLE
from topos.evaluation.policies.gates import evaluate_gates, interpret_metric


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
    metrics = {
        key: value
        for key, value in {
            "cfg.cyclomatic": cyclomatic,
            "ast.entropy": entropy,
            "ast.max_function_complexity": max_function_complexity,
        }.items()
        if value is not None
    }
    results = evaluate_gates(
        metrics, pillar="simple", is_entrypoint_module=is_entrypoint_module
    )
    if not results:
        # If no metrics are provided, we vacuously satisfy SIMPLE.
        return ScoredDecision(score=1.0, achieved=True, interpretation={})

    # Score shaping (reporting only): quality curves stay local to Φ_SIMPLE.
    qualities = [_quality(r.spec.metric, r.value) for r in results]

    return ScoredDecision(
        # The combined score is the minimum of the individual qualities
        # (conservative AND).
        score=min(qualities),
        achieved=all(r.passed for r in results),
        interpretation={r.spec.metric: r.interpretation for r in results},
    )


def _quality(metric: str, value: float) -> float:
    """Normalize one raw metric to a [0, 1] quality (never gates achieved)."""
    if metric == "cfg.cyclomatic":
        return 1.0 - min(value / SIMPLE.max_cyclomatic_cap, 1.0)
    if metric == "ast.entropy":
        return max(0.0, 1.0 - 2.0 * abs(value - SIMPLE.entropy_ideal))
    return 1.0 - min(value / SIMPLE.max_function_complexity_cap, 1.0)


def build_omega() -> Omega:
    """Build the subobject classifier Ω = H(G_qual) (8-element 3-cube)."""
    return Omega()


def describe_entropy_ratio(entropy: float) -> str:
    """Describe a raw AST entropy ratio using SIMPLE policy language."""
    return interpret_metric("ast.entropy", entropy)
