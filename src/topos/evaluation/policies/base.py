"""
Shared scoring infrastructure for the policy translators Φᵢ : ℝ → Ω.

Following the math spec (§3 "Policy Translation"), each quality
generator ``gᵢ ∈ G_qual`` has an associated policy translator ``Φᵢ`` that
maps real-valued probe outputs (cyclomatic complexity, Martin
instability, taint-flow counts, …) into the truth-value carrier of Ω.

This module defines the shared types used by every Φᵢ:

- :data:`THRESHOLDS`      — per-generator strict thresholds.
- :class:`Priority`       — single-generator emphasis (legacy knob;
                            see :class:`topos.evaluation.preferences.UserPreferences`
                            for the full strict ordering).
- :class:`WeightProfile`  — per-generator intra-dimension metric weights.
- :class:`ScoredDecision` — the output of a single Φᵢ.

There is exactly one ``Φᵢ`` per generator::

    Φ_SIMPLE      ↦ topos/evaluation/policies/simple.py::score_simple
    Φ_COMPOSABLE  ↦ topos/evaluation/policies/coupling.py::score_coupling
    Φ_SECURE      ↦ topos/evaluation/policies/secure.py::score_secure

Decisive semantics: AND-of-thresholds
=====================================
Each generator gets a **single strict threshold**.  An evaluation
either *is* SIMPLE or it is not — and similarly, independently, for
COMPOSABLE and SECURE::

    is_simple      = score_simple      ≥ THRESHOLDS[SIMPLE]
    is_composable  = score_composable  ≥ THRESHOLDS[COMPOSABLE]
    is_secure      = score_secure      ≥ THRESHOLDS[SECURE]

The 8-element verdict in Ω is then the **independent AND** of those
three checks — the lattice meets fall out for free::

    SIMPLE_COMPOSABLE  ⟺  is_simple ∧ is_composable                 (¬ is_secure)
    SIMPLE_SECURE      ⟺  is_simple ∧ is_secure                    (¬ is_composable)
    COMPOSABLE_SECURE  ⟺  is_composable ∧ is_secure                (¬ is_simple)
    IDEAL              ⟺  is_simple ∧ is_composable ∧ is_secure
    SLOP               ⟺  ¬ is_simple ∧ ¬ is_composable ∧ ¬ is_secure

Crucially the threshold checks are **independent**.  The changes
needed to satisfy SIMPLE (split branches, lift guard clauses) are
orthogonal to those needed for SECURE (eliminate taint paths) or
COMPOSABLE (rebalance fan-in/fan-out).  Two of these may be cheap to
fix on a given file while the third is structurally hard — which is
exactly why agents *walk down the relaxation tree* (see
:mod:`topos.evaluation.preferences`): aspire to IDEAL, then divert to
the user-preferred pairwise meet, then to a single satisfied
generator, and so on.

Preferences, always
===================
The manager **must** supply a preference.  There is no catch-all
"no opinion" mode any more — every evaluation pins a generator (via
:class:`Priority`) or a full strict total order (via
:class:`topos.evaluation.preferences.UserPreferences`).  ``Priority``
captures only the top-ranked generator; ``UserPreferences`` captures
the full ranking and drives the relaxation walk on Ω.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from enum import StrEnum

from topos.evaluation.preferences import Generator

# ---------------------------------------------------------------------------
# Thresholds — what counts as "this generator is satisfied"
# ---------------------------------------------------------------------------
#
# These are *strict* per-generator thresholds: ``score ≥ THRESHOLDS[g]``
# is the entire criterion.  The 8 lattice verdicts emerge from the
# independent AND of the three checks.
#
# Defaults chosen to be *reasonable but uncalibrated*:
#   - SIMPLE      0.60   complexity is cheap to verify and noisy at the low
#                        end; 60% is a comfortable mid-bar.
#   - COMPOSABLE  0.60   coupling/instability are interpolated from Martin's
#                        ratios — same bar as SIMPLE.
#   - SECURE      0.70   taint reachability is structural and high-stakes;
#                        the false-negative cost is asymmetrically bad.
#                        Hold security to a tighter bar.
#
# NOTE: These will be re-calibrated against a corpus of known libraries
# (CPython stdlib, requests, pydantic, …) once the calibration harness
# lands.  Treat them as v0 placeholders, not hard constants.

THRESHOLDS: dict[Generator, float] = {
    Generator.SIMPLE: 0.60,
    Generator.COMPOSABLE: 0.60,
    Generator.SECURE: 0.70,
}


def threshold(generator: Generator) -> float:
    """The strict score threshold for one generator.

    ``score ≥ threshold(g)`` ⟺ ``g`` is satisfied.
    """
    return THRESHOLDS[generator]


def is_satisfied(generator: Generator, score: float) -> bool:
    """Independent threshold check for one generator."""
    return score >= THRESHOLDS[generator]


def meet_satisfied(scores: dict[Generator, float]) -> dict[Generator, bool]:
    """Apply the strict thresholds independently across all supplied scores.

    Returns a mapping ``{g: is_satisfied(g, scores[g])}``.  Callers
    feed this directly into ``verdict_from_generators`` to land on the
    matching element of Ω — meets are *literally* the AND of these
    booleans, no fancy aggregation.

    This is the canonical place that encodes the orthogonality of the
    three generators: each score gets its own independent yes/no, and
    the lattice element is just the bitmask of those answers.
    """
    return {g: is_satisfied(g, scores.get(g, 0.0)) for g in Generator}


# ---------------------------------------------------------------------------
# Priority (legacy / single-generator emphasis)
# ---------------------------------------------------------------------------


class Priority(StrEnum):
    """Single-generator emphasis.

    A ``Priority`` is the lower-resolution shadow of a full
    :class:`~topos.evaluation.preferences.UserPreferences`: it
    captures only the **top-ranked generator** of the ordering.  New
    code should pass a ``UserPreferences`` ranking; ``Priority`` is
    the simpler CLI / single-knob shorthand when the caller does not
    want to articulate a full strict order.

    Members:
        SIMPLE:      The user's top-ranked generator is SIMPLE.
        COMPOSABLE:  The user's top-ranked generator is COMPOSABLE.
        SECURE:      The user's top-ranked generator is SECURE.

    Default across the codebase is ``SIMPLE`` — the most conservative
    single choice, matching :func:`topos.evaluation.preferences.default_preferences`.
    """

    SIMPLE = "simple"
    COMPOSABLE = "composable"
    SECURE = "secure"

    def top_generator(self) -> Generator:
        """The generator this priority emphasizes."""
        return Generator(self.value)


# ---------------------------------------------------------------------------
# WeightProfile — intra-Φᵢ weights between each generator's metrics
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class WeightProfile:
    """Per-generator metric weights for a given priority/ranking.

    Each weight controls the linear combination *within* one Φᵢ
    between its two principal metrics.  The two weights inside a
    single ``WeightProfile`` are independent across dimensions — they
    do not sum to 1 across generators.

    Attributes:
        w_complexity:  Weight on cyclomatic_quality within Φ_SIMPLE.
                       Entropy gets ``1 - w_complexity``.
        w_coupling:    Weight on coupling_quality within Φ_COMPOSABLE.
                       Instability gets ``1 - w_coupling``.
        w_taint:       Weight on taint_quality within Φ_SECURE.
                       Dangerous-API reachability gets ``1 - w_taint``.
    """

    w_complexity: float
    w_coupling: float
    w_taint: float

    # ------------------------------------------------------------------ #
    # Constructors                                                        #
    # ------------------------------------------------------------------ #

    @classmethod
    def from_ranking(
        cls, ranking: tuple[Generator, Generator, Generator]
    ) -> WeightProfile:
        """Derive intra-policy weights from a full preference ordering.

        Top-ranked generator's Φᵢ is biased toward its primary metric
        (so it is more decisive); the bottom-ranked one's stays
        conservative.  Concretely we use weights ``0.7 / 0.5 / 0.3``
        in ranking order::

            ranking[0] (top)    → primary-metric weight 0.7  (decisive)
            ranking[1] (middle) → primary-metric weight 0.5  (neutral)
            ranking[2] (bottom) → primary-metric weight 0.3  (conservative)

        This is the canonical path from a
        :class:`~topos.evaluation.preferences.UserPreferences` ranking
        to scoring weights.
        """
        if set(ranking) != set(Generator) or len(ranking) != 3:
            raise ValueError(
                f"ranking must be a permutation of G_qual, got {ranking!r}"
            )
        rank_weight = {ranking[0]: 0.7, ranking[1]: 0.5, ranking[2]: 0.3}
        return cls(
            w_complexity=rank_weight[Generator.SIMPLE],
            w_coupling=rank_weight[Generator.COMPOSABLE],
            w_taint=rank_weight[Generator.SECURE],
        )

    @classmethod
    def from_priority(cls, priority: Priority) -> WeightProfile:
        """Look up the legacy ``Priority``-keyed weight profile."""
        return WEIGHT_PROFILES[priority]


WEIGHT_PROFILES: dict[Priority, WeightProfile] = {
    Priority.SIMPLE: WeightProfile(w_complexity=0.7, w_coupling=0.3, w_taint=0.3),
    Priority.COMPOSABLE: WeightProfile(w_complexity=0.3, w_coupling=0.7, w_taint=0.3),
    Priority.SECURE: WeightProfile(w_complexity=0.3, w_coupling=0.3, w_taint=0.7),
}


# ---------------------------------------------------------------------------
# ScoredDecision — the output of one Φᵢ
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class ScoredDecision:
    """Result of applying one policy translator Φᵢ : ℝ → Ω.

    Attributes:
        score:          Quality score in [0.0, 1.0]; higher is better.
        achieved:       True when ``score >= threshold`` — i.e. the
                        generator gᵢ is satisfied for this program.
                        This is the *only* fact the lattice combinator
                        uses; meets in Ω are just the AND of these
                        booleans across generators.
        interpretation: Per-metric human-readable strings keyed by
                        metric name (e.g. ``cfg.cyclomatic``).
    """

    score: float
    achieved: bool
    interpretation: dict[str, str] = field(default_factory=dict)
