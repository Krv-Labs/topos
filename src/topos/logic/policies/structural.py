"""
Structural Evaluation Scorer: Observation Space -> Subobject Classifier
-----------------------------------------------------------------------

Maps AST-based metric observations (cyclomatic complexity, Kolmogorov-proxy
entropy) into a continuous quality score that determines whether the
SELF_CONTAINED lattice target is achieved.

The factorization is:

    Program --(ast metrics)--> R^2 --(scorer)--> [0, 1] --(threshold)--> Omega

``score_structural`` is the interpretive layer: it converts raw measurements
into a normalized quality score in [0, 1], where 1.0 is ideal.  Moving the
normalization bounds or the threshold is a policy decision; this module is
where those decisions live.

Quality functions:
    complexity_quality  = 1 - min(complexity / MAX_COMPLEXITY, 1.0)
                          Linear fall from 1.0 at complexity=0 to 0.0 at MAX.

    entropy_quality     = max(0, 1 - 2 * |entropy - ENTROPY_IDEAL|)
                          Bell-curve peak at ENTROPY_IDEAL, reaching 0.0 at
                          the extremes (0.0 or 1.0).

The weighted structural score = w_c * complexity_quality + (1-w_c) * entropy_quality
where w_c comes from the Priority's WeightProfile.
"""

from __future__ import annotations

from topos.logic.lattice import EvaluationLattice
from topos.logic.policies.base import (
    WEIGHT_PROFILES,
    Priority,
    ScoredDecision,
)

# Normalization constants (policy decisions)
MAX_COMPLEXITY: float = 40.0  # complexity at which quality reaches 0.0
ENTROPY_IDEAL: float = 0.5  # entropy value with maximum quality score


def score_structural(
    complexity: float,
    entropy: float,
    priority: Priority,
    threshold: float = 0.6,
) -> ScoredDecision:
    """
    Score the structural quality of a program.

    Args:
        complexity: Cyclomatic complexity (raw integer count).
        entropy:    Kolmogorov-proxy compression ratio (typically in [0, 2]).
        priority:   Weight profile controlling complexity vs entropy emphasis.
        threshold:  Minimum score to achieve the SELF_CONTAINED lattice target.

    Returns:
        A ScoredDecision with a [0, 1] quality score and per-metric
        interpretation strings.
    """
    complexity_quality = 1.0 - min(complexity / MAX_COMPLEXITY, 1.0)
    entropy_quality = max(0.0, 1.0 - 2.0 * abs(entropy - ENTROPY_IDEAL))

    w_c = WEIGHT_PROFILES[priority].w_complexity
    structural_score = w_c * complexity_quality + (1.0 - w_c) * entropy_quality

    return ScoredDecision(
        score=structural_score,
        achieved=structural_score >= threshold,
        interpretation={
            "ast.complexity": _complexity_interpretation(complexity_quality),
            "ast.entropy": _entropy_interpretation(entropy, entropy_quality),
        },
    )


def build_evaluation_lattice() -> EvaluationLattice:
    """Build the evaluation lattice (diamond Heyting algebra)."""
    return EvaluationLattice()


# ---------------------------------------------------------------------------
# Private interpretation helpers
# ---------------------------------------------------------------------------


def _complexity_interpretation(quality: float) -> str:
    if quality >= 0.75:
        return "complexity within expected range"
    if quality >= 0.5:
        return "complexity is elevated but manageable"
    if quality >= 0.25:
        return "complexity is high and branching-heavy"
    return "complexity is pathologically high"


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
