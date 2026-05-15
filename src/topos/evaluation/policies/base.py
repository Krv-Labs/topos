"""
Shared types for the policy translators Φᵢ : ℝ → Ω.

Following the math spec (§3 "Policy Translation"), each quality
generator ``gᵢ ∈ G_qual`` has an associated policy translator ``Φᵢ``
that maps probe outputs into a :class:`ScoredDecision`.  The
characteristic morphism (:mod:`topos.evaluation.characteristic_morphism`)
reads each decision's ``achieved`` flag and assembles the 8-element
verdict in Ω via :func:`topos.core.omega.verdict_from_generators`.

This module defines the shared types; the decisive thresholds live in
each Φᵢ module:

- :class:`ScoredDecision` — output of one Φᵢ (score + achieved + text).
- :class:`Priority`       — legacy single-generator emphasis (signature
                            compatibility only; Φᵢ do not use it today).
- :class:`WeightProfile`  — legacy intra-Φᵢ metric weights (same).
- :data:`THRESHOLDS`      — optional score-floor helpers for tools that
                            aggregate normalized scores without re-running
                            a full Φᵢ.

There is exactly one ``Φᵢ`` per generator::

    Φ_SIMPLE      ↦ simple.py::score_simple
    Φ_COMPOSABLE  ↦ composable.py::score_coupling
    Φ_SECURE      ↦ secure.py::score_secure

Auxiliary policies (outside G_qual / Ω) live alongside these::

    clone detection   ↦ clones.py::are_clones
    test coverage     ↦ coverage.py::score_declaration_coverage

Decisive semantics: AND-of-raw-metric thresholds
================================================
Each ``Φᵢ`` owns **per-metric raw thresholds** (cyclomatic ≤ 15, zero
taint flows, fan-in ≤ 15, …).  ``achieved`` is the independent AND
of those checks — *not* ``score ≥ THRESHOLDS[g]``::

    achieved_simple     = score_simple(...).achieved
    achieved_composable = score_coupling(...).achieved
    achieved_secure     = score_secure(...).achieved

The normalized ``score`` on :class:`ScoredDecision` is
``min(per-metric qualities)`` for reporting and multi-file meets; it
does not gate ``achieved``.

The 8-element verdict in Ω is the **independent AND** of the three
``achieved`` flags — lattice meets fall out for free::

    SIMPLE_COMPOSABLE  ⟺  achieved_simple ∧ achieved_composable
    IDEAL              ⟺  all three achieved
    SLOP               ⟺  none achieved
    …

Generator checks are **orthogonal**: fixes for SIMPLE (split branches,
lift guards) do not substitute for SECURE (eliminate taint) or
COMPOSABLE (rebalance coupling).  Agents walk the relaxation tree in
:mod:`topos.evaluation.preferences` when IDEAL is unreachable.

Score-floor helpers (:data:`THRESHOLDS`, :func:`meet_satisfied`)
================================================================
:data:`THRESHOLDS` and :func:`is_satisfied` / :func:`meet_satisfied`
implement an alternate **score-floor** gate (``score ≥ THRESHOLDS[g]``)
for callers that already hold normalized scores.  The live
:class:`~topos.evaluation.characteristic_morphism.CharacteristicMorphism`
path does **not** use them — it trusts ``ScoredDecision.achieved`` from
each ``Φᵢ``.

Preferences
===========
Full strict orderings on generators live in
:class:`topos.evaluation.preferences.UserPreferences` (relaxation walk,
aspirational IDEAL vs pragmatic pairwise meet).  :class:`Priority` is the
lower-resolution CLI shorthand for the top-ranked generator only.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from enum import StrEnum

from topos.evaluation.preferences import Generator

# ---------------------------------------------------------------------------
# Score-floor thresholds (alternate path — not used by Φᵢ translators)
# ---------------------------------------------------------------------------
#
# Φᵢ modules gate ``achieved`` on raw probe values.  These normalized
# score floors are for :func:`meet_satisfied` and other callers that
# already aggregated scores without re-invoking a Φᵢ.
#
# Defaults are v0 placeholders pending corpus calibration.

THRESHOLDS: dict[Generator, float] = {
    Generator.SIMPLE: 0.60,
    Generator.COMPOSABLE: 0.60,
    Generator.SECURE: 0.70,
}


def threshold(generator: Generator) -> float:
    """Normalized score floor for one generator (score-floor path only)."""
    return THRESHOLDS[generator]


def is_satisfied(generator: Generator, score: float) -> bool:
    """Whether a normalized score clears the score-floor for one generator."""
    return score >= THRESHOLDS[generator]


def meet_satisfied(scores: dict[Generator, float]) -> dict[Generator, bool]:
    """Score-floor AND across generators, for pre-aggregated normalized scores.

    Returns ``{g: score[g] ≥ THRESHOLDS[g]}``.  Feed into
    :func:`topos.core.omega.verdict_from_generators` for the Ω element.

    Prefer each ``Φᵢ``'s ``ScoredDecision.achieved`` when probe metrics are
    available — that path applies raw-metric thresholds defined in
    :mod:`topos.evaluation.policies.simple`, ``composable``, and ``secure``.
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

    Passed through the classify API for compatibility; current ``Φᵢ``
    implementations do not change ``achieved`` based on priority.
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
    """Legacy per-generator metric weights for a priority/ranking.

    Retained for :meth:`from_ranking` / :data:`WEIGHT_PROFILES` and API
    stability.  Current ``Φᵢ`` implementations use fixed AND-of-raw-thresholds
    and do not read these weights.

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
    """Result of applying one policy translator Φᵢ.

    Attributes:
        score:          Conservative ``min(per-metric qualities)`` in
                        [0.0, 1.0] for display and multi-file aggregation.
                        Does **not** gate ``achieved``.
        achieved:       True when every supplied raw metric passes that
                        Φᵢ's policy thresholds (AND semantics).  This is
                        what :class:`~topos.evaluation.characteristic_morphism.CharacteristicMorphism`
                        feeds into ``verdict_from_generators``.
        interpretation: Per-metric human-readable strings keyed by
                        metric name (e.g. ``cfg.cyclomatic``).
    """

    score: float
    achieved: bool
    interpretation: dict[str, str] = field(default_factory=dict)
