"""
User preferences over the quality generators — induced order on Ω.
==================================================================

``Priority`` (in :mod:`topos.evaluation.policies.base`) is a *single*
upweighted generator: a knob on the policy translators ``Φᵢ``.  This
module is a strictly stronger statement of the manager's intent — a
**strict total order on the three generators**::

    g₁ ≻ g₂ ≻ g₃   with   {g₁, g₂, g₃} = G_qual

The lattice Ω = H(G_qual) is *partially* ordered (the three generator
atoms are pairwise incomparable).  A user preference linearizes it.  We
score each verdict ``v ∈ Ω`` by the satisfied-generator bitmask weighted
by preference rank::

    score(v) = Σᵢ 2^(n − i) · ⟦gᵢ satisfied by v⟧

So with the ranking ``(SIMPLE, COMPOSABLE, SECURE)`` (most → least)::

    IDEAL              = 4 + 2 + 1 = 7
    SIMPLE_COMPOSABLE  = 4 + 2     = 6      ← default target ("ideal ∩")
    SIMPLE_SECURE      = 4 + 1     = 5
    SIMPLE             = 4
    COMPOSABLE_SECURE  =     2 + 1 = 3
    COMPOSABLE         =     2
    SECURE             =         1
    SLOP               = 0

The strict total order this induces refines Ω's Heyting order: ``a ≤_H
b ⟹ a ⪯_r b``.  Where the Heyting order leaves atoms incomparable, the
preference order disambiguates.

Aspirational target + pragmatic fallback
----------------------------------------
The walk is **two-stage**:

1. **Aspirational target** — IDEAL.  Agents first try to beat the
   policy thresholds for *all three* generators.  Topos doesn't assume
   IDEAL is unreachable a priori; some files genuinely satisfy every
   generator.
2. **Pragmatic target (the "ideal intersection")** — the meet of the
   *top-two* ranked generators.  If IDEAL stops moving after a few
   iterations, the agent **diverts** to this pairwise meet.  For
   ranking ``(SIMPLE, COMPOSABLE, SECURE)`` the fallback is
   ``SIMPLE_COMPOSABLE``; for ``(COMPOSABLE, SECURE, SIMPLE)`` it is
   ``COMPOSABLE_SECURE``; etc.

Relaxation walk
---------------
``relaxation_walk(prefs, current)`` returns the descending sequence of
verdicts from **IDEAL** down to (but not including) the current
verdict — the **targeted relaxation walk**.  The agent uses it to pick
the next achievable goal one step at a time; the pragmatic target sits
exactly one step below IDEAL in the walk and is the natural
divert-point when IDEAL plateaus.
"""

from __future__ import annotations

from collections.abc import Iterable
from dataclasses import dataclass
from enum import StrEnum

from topos.core.omega import EvaluationValue, verdict_from_generators


class Generator(StrEnum):
    """The three quality generators of ``G_qual``."""

    SIMPLE = "simple"
    COMPOSABLE = "composable"
    SECURE = "secure"


_GENERATOR_BIT: dict[Generator, int] = {
    Generator.SIMPLE: 0b001,
    Generator.COMPOSABLE: 0b010,
    Generator.SECURE: 0b100,
}


def _generator_satisfied(value: EvaluationValue, g: Generator) -> bool:
    return bool(int(value) & _GENERATOR_BIT[g])


@dataclass(frozen=True)
class UserPreferences:
    """A strict total order on ``G_qual``.

    Attributes:
        ranking: Three distinct generators, most-preferred first.
        target:  Optional explicit aspirational target verdict.
                 Defaults to ``IDEAL`` — the agent first tries to beat
                 the policy thresholds for all three generators.  When
                 IDEAL plateaus, ``fallback_target`` (the meet of the
                 top-two ranked generators) is the natural divert
                 point.
    """

    ranking: tuple[Generator, Generator, Generator]
    target: EvaluationValue | None = None

    def __post_init__(self) -> None:
        if len(self.ranking) != 3 or set(self.ranking) != set(Generator):
            raise ValueError(
                f"ranking must be a permutation of {{simple, composable, secure}}, "
                f"got {self.ranking!r}"
            )

    @classmethod
    def from_iterable(
        cls,
        ranking: Iterable[Generator | str],
        *,
        target: EvaluationValue | None = None,
    ) -> UserPreferences:
        coerced = tuple(Generator(r) for r in ranking)
        if len(coerced) != 3:
            raise ValueError(f"ranking must have length 3, got {coerced!r}")
        return cls(ranking=coerced, target=target)  # type: ignore[arg-type]

    # ------------------------------------------------------------------ #
    # Induced ordering                                                    #
    # ------------------------------------------------------------------ #

    def score(self, value: EvaluationValue) -> int:
        """Lex-weighted preference score for a verdict.

        Higher is more preferred.  Weights are 4 / 2 / 1 across the
        ranking so the top-ranked generator dominates the next two
        combined — strictly lexicographic on the satisfied-generator
        bits in preference order.
        """
        weights = (4, 2, 1)
        return sum(
            w if _generator_satisfied(value, g) else 0
            for w, g in zip(weights, self.ranking, strict=True)
        )

    def induced_total_order(self) -> list[EvaluationValue]:
        """All 8 verdicts sorted by descending preference."""
        return sorted(EvaluationValue, key=self.score, reverse=True)

    # ------------------------------------------------------------------ #
    # Target + relaxation walk                                            #
    # ------------------------------------------------------------------ #

    def aspirational_target(self) -> EvaluationValue:
        """The first target the agent should attempt.

        Defaults to ``IDEAL`` (beat the policy thresholds for all three
        generators).  Override via the ``target`` field if the caller
        knows a priori that IDEAL is unreachable for this codebase.
        """
        return self.target if self.target is not None else EvaluationValue.IDEAL

    def fallback_target(self) -> EvaluationValue:
        """The pragmatic divert-point if IDEAL plateaus.

        This is the meet of the top-two ranked generators — what we
        call the **"ideal intersection"**.  For ranking
        ``(COMPOSABLE, SECURE, SIMPLE)`` this is ``COMPOSABLE_SECURE``;
        for ``(SIMPLE, COMPOSABLE, SECURE)`` it is
        ``SIMPLE_COMPOSABLE``.
        """
        g1, g2, _ = self.ranking
        return verdict_from_generators(
            simple=Generator.SIMPLE in (g1, g2),
            composable=Generator.COMPOSABLE in (g1, g2),
            secure=Generator.SECURE in (g1, g2),
        )

    # Backwards-compatible alias — the "resolved" target is what the
    # agent aims at on iteration 1.  Always IDEAL unless overridden.
    def resolved_target(self) -> EvaluationValue:
        return self.aspirational_target()

    def relaxation_walk(
        self, current: EvaluationValue | None = None
    ) -> list[EvaluationValue]:
        """Descending walk from the aspirational target toward ``current``.

        Returned in descending preference order starting at the
        aspirational target (default: ``IDEAL``) and ending one step
        above ``current``.  The **second** element of the walk is the
        ``fallback_target`` — the natural divert-point when IDEAL
        proves unreachable.

        Empty when ``current`` already meets or exceeds the target.
        """
        target = self.aspirational_target()
        target_score = self.score(target)
        order = self.induced_total_order()
        descending = [v for v in order if self.score(v) <= target_score]

        if current is None:
            return descending

        current_score = self.score(current)
        if current_score >= target_score:
            return []
        return [v for v in descending if self.score(v) > current_score]

    def next_step(self, current: EvaluationValue) -> EvaluationValue | None:
        """The immediate next achievable verdict above ``current``.

        Bottom of the relaxation walk — the smallest improvement that
        still respects the preference order.  ``None`` when at or
        beyond the aspirational target.
        """
        walk = self.relaxation_walk(current)
        if not walk:
            return None
        return walk[-1]

    def progress(self, current: EvaluationValue) -> float:
        """Fractional progress from ``SLOP`` to the aspirational target.

        Returns a value in ``[0.0, 1.0]``.  Reaches ``1.0`` exactly at
        the target verdict.
        """
        target_score = self.score(self.aspirational_target())
        if target_score == 0:
            return 1.0
        return min(1.0, self.score(current) / target_score)


# ---------------------------------------------------------------------- #
# Convenience                                                             #
# ---------------------------------------------------------------------- #


def default_preferences() -> UserPreferences:
    """Conservative default: ``SIMPLE ≻ COMPOSABLE ≻ SECURE``.

    Simplicity comes first (the cheapest property to verify and currently our
    strongest measure), then composability (the most cross-cutting and the only
    one requiring an external dep graph), then security.
    """
    return UserPreferences(
        ranking=(Generator.SIMPLE, Generator.COMPOSABLE, Generator.SECURE),
    )
