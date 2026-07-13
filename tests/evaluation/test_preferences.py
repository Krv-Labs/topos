"""Tests for ``topos.evaluation.preferences``."""

from __future__ import annotations

import pytest
from topos.core.omega import EvaluationValue, all_evaluation_values
from topos.evaluation.preferences import (
    Generator,
    UserPreferences,
    default_preferences,
)


def _prefs(*ranking: Generator) -> UserPreferences:
    return UserPreferences(ranking=tuple(ranking))  # type: ignore[arg-type]


def test_ranking_must_be_permutation():
    with pytest.raises(ValueError):
        UserPreferences(ranking=(Generator.SIMPLE, Generator.SIMPLE, Generator.SECURE))  # type: ignore[arg-type]


def test_aspirational_target_is_ideal_by_default():
    prefs = _prefs(Generator.SIMPLE, Generator.COMPOSABLE, Generator.SECURE)
    assert prefs.aspirational_target() == EvaluationValue.IDEAL


def test_fallback_target_is_top_two_meet():
    prefs = _prefs(Generator.SIMPLE, Generator.COMPOSABLE, Generator.SECURE)
    assert prefs.fallback_target() == EvaluationValue.SIMPLE_COMPOSABLE

    prefs = _prefs(Generator.SECURE, Generator.SIMPLE, Generator.COMPOSABLE)
    assert prefs.fallback_target() == EvaluationValue.SIMPLE_SECURE

    prefs = _prefs(Generator.COMPOSABLE, Generator.SECURE, Generator.SIMPLE)
    assert prefs.fallback_target() == EvaluationValue.COMPOSABLE_SECURE


def test_explicit_target_override():
    prefs = UserPreferences(
        ranking=(Generator.SIMPLE, Generator.COMPOSABLE, Generator.SECURE),
        target=EvaluationValue.SIMPLE_COMPOSABLE,
    )
    assert prefs.aspirational_target() == EvaluationValue.SIMPLE_COMPOSABLE


def test_induced_order_is_lex_on_weights():
    prefs = _prefs(Generator.SIMPLE, Generator.COMPOSABLE, Generator.SECURE)
    order = prefs.induced_total_order()
    # Highest = IDEAL, then SIMPLE_COMPOSABLE, then SIMPLE_SECURE, …
    assert order[0] == EvaluationValue.IDEAL
    assert order[1] == EvaluationValue.SIMPLE_COMPOSABLE
    assert order[2] == EvaluationValue.SIMPLE_SECURE
    assert order[3] == EvaluationValue.SIMPLE
    assert order[-1] == EvaluationValue.SLOP


def test_induced_order_refines_heyting():
    """``a ≤_H b ⟹ a ⪯_r b`` for any ranking."""
    from topos.core.omega import Omega

    omega = Omega()
    for ranking in (
        (Generator.SIMPLE, Generator.COMPOSABLE, Generator.SECURE),
        (Generator.SECURE, Generator.COMPOSABLE, Generator.SIMPLE),
        (Generator.COMPOSABLE, Generator.SIMPLE, Generator.SECURE),
    ):
        prefs = _prefs(*ranking)
        for a in all_evaluation_values():
            for b in all_evaluation_values():
                if omega.leq(a, b):
                    assert prefs.score(a) <= prefs.score(b)


def test_relaxation_walk_starts_at_ideal_then_fallback():
    prefs = _prefs(Generator.SIMPLE, Generator.COMPOSABLE, Generator.SECURE)
    walk = prefs.relaxation_walk(EvaluationValue.SLOP)
    # IDEAL is the aspirational target — first in the walk.
    assert walk[0] == EvaluationValue.IDEAL
    # The "divert" element directly below IDEAL is the fallback target.
    assert walk[1] == EvaluationValue.SIMPLE_COMPOSABLE
    assert walk[1] == prefs.fallback_target()


def test_relaxation_walk_stops_above_current():
    prefs = _prefs(Generator.SIMPLE, Generator.COMPOSABLE, Generator.SECURE)
    walk = prefs.relaxation_walk(EvaluationValue.SIMPLE)
    # All walk entries must outrank the current verdict.
    for v in walk:
        assert prefs.score(v) > prefs.score(EvaluationValue.SIMPLE)
    # IDEAL and the fallback both included.
    assert EvaluationValue.IDEAL in walk
    assert EvaluationValue.SIMPLE_COMPOSABLE in walk
    # SECURE / COMPOSABLE_SECURE / COMPOSABLE / SLOP rank below SIMPLE.
    assert EvaluationValue.SECURE not in walk
    assert EvaluationValue.SLOP not in walk


def test_relaxation_walk_empty_at_ideal():
    prefs = _prefs(Generator.SIMPLE, Generator.COMPOSABLE, Generator.SECURE)
    assert prefs.relaxation_walk(EvaluationValue.IDEAL) == []
    assert prefs.next_step(EvaluationValue.IDEAL) is None


def test_next_step_is_smallest_improvement():
    prefs = _prefs(Generator.SIMPLE, Generator.COMPOSABLE, Generator.SECURE)
    # From SLOP the smallest improvement is the lowest-ranked verdict
    # strictly above SLOP — which is SECURE (weight 1).
    assert prefs.next_step(EvaluationValue.SLOP) == EvaluationValue.SECURE
    # From SECURE → COMPOSABLE (weight 2).
    assert prefs.next_step(EvaluationValue.SECURE) == EvaluationValue.COMPOSABLE


def test_progress_reaches_one_at_ideal():
    prefs = _prefs(Generator.SIMPLE, Generator.COMPOSABLE, Generator.SECURE)
    assert prefs.progress(EvaluationValue.SLOP) == 0.0
    assert prefs.progress(EvaluationValue.IDEAL) == 1.0
    # Fallback target is partial progress (6/7 with weights 4+2+1).
    assert 0.8 < prefs.progress(EvaluationValue.SIMPLE_COMPOSABLE) < 1.0


def test_default_preferences():
    prefs = default_preferences()
    assert prefs.ranking[0] == Generator.SIMPLE
    assert prefs.aspirational_target() == EvaluationValue.IDEAL
    assert prefs.fallback_target() == EvaluationValue.SIMPLE_COMPOSABLE
