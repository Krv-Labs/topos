"""Tests for the policies base layer — thresholds + ranking-derived weights."""

from __future__ import annotations

import pytest

from topos.core.omega import EvaluationValue, verdict_from_generators
from topos.evaluation.policies.base import (
    THRESHOLDS,
    WEIGHT_PROFILES,
    Priority,
    WeightProfile,
    is_satisfied,
    meet_satisfied,
    threshold,
)
from topos.evaluation.preferences import Generator

# ---------------------------------------------------------------------- #
# Thresholds                                                              #
# ---------------------------------------------------------------------- #


def test_every_generator_has_a_threshold():
    for g in Generator:
        assert 0.0 < THRESHOLDS[g] <= 1.0


def test_threshold_lookup_matches_dict():
    for g in Generator:
        assert threshold(g) == THRESHOLDS[g]


def test_secure_threshold_is_strictest_by_default():
    """SECURE's false-negative cost is asymmetric — calibration v0."""
    assert THRESHOLDS[Generator.SECURE] >= THRESHOLDS[Generator.SIMPLE]
    assert THRESHOLDS[Generator.SECURE] >= THRESHOLDS[Generator.COMPOSABLE]


def test_is_satisfied_uses_strict_geq():
    th = THRESHOLDS[Generator.SIMPLE]
    assert is_satisfied(Generator.SIMPLE, th) is True
    assert is_satisfied(Generator.SIMPLE, th - 1e-9) is False
    assert is_satisfied(Generator.SIMPLE, 1.0) is True
    assert is_satisfied(Generator.SIMPLE, 0.0) is False


# ---------------------------------------------------------------------- #
# Meet semantics: AND of independent threshold checks                     #
# ---------------------------------------------------------------------- #


def test_meet_satisfied_is_independent_and():
    scores = {
        Generator.SIMPLE: 0.9,
        Generator.COMPOSABLE: 0.9,
        Generator.SECURE: 0.0,
    }
    result = meet_satisfied(scores)
    assert result == {
        Generator.SIMPLE: True,
        Generator.COMPOSABLE: True,
        Generator.SECURE: False,
    }
    # Combining via verdict_from_generators lands on the matching meet.
    verdict = verdict_from_generators(
        simple=result[Generator.SIMPLE],
        composable=result[Generator.COMPOSABLE],
        secure=result[Generator.SECURE],
    )
    assert verdict == EvaluationValue.SIMPLE_COMPOSABLE


def test_meet_satisfied_handles_missing_scores_as_zero():
    result = meet_satisfied({Generator.SIMPLE: 0.99})
    assert result[Generator.SIMPLE] is True
    assert result[Generator.COMPOSABLE] is False
    assert result[Generator.SECURE] is False


def test_all_satisfied_yields_ideal():
    scores = {g: 1.0 for g in Generator}
    sat = meet_satisfied(scores)
    verdict = verdict_from_generators(
        simple=sat[Generator.SIMPLE],
        composable=sat[Generator.COMPOSABLE],
        secure=sat[Generator.SECURE],
    )
    assert verdict == EvaluationValue.IDEAL


def test_none_satisfied_yields_slop():
    scores = {g: 0.0 for g in Generator}
    sat = meet_satisfied(scores)
    verdict = verdict_from_generators(
        simple=sat[Generator.SIMPLE],
        composable=sat[Generator.COMPOSABLE],
        secure=sat[Generator.SECURE],
    )
    assert verdict == EvaluationValue.SLOP


# ---------------------------------------------------------------------- #
# WeightProfile.from_ranking                                              #
# ---------------------------------------------------------------------- #


def test_from_ranking_top_generator_gets_highest_weight():
    profile = WeightProfile.from_ranking(
        (Generator.SIMPLE, Generator.COMPOSABLE, Generator.SECURE)
    )
    # SIMPLE is top → its primary metric (complexity) gets 0.7.
    assert profile.w_complexity == 0.7
    # COMPOSABLE is middle → 0.5.
    assert profile.w_coupling == 0.5
    # SECURE is bottom → 0.3.
    assert profile.w_taint == 0.3


def test_from_ranking_bottom_generator_gets_conservative_weight():
    profile = WeightProfile.from_ranking(
        (Generator.SECURE, Generator.SIMPLE, Generator.COMPOSABLE)
    )
    assert profile.w_taint == 0.7  # SECURE top → decisive
    assert profile.w_complexity == 0.5  # SIMPLE middle
    assert profile.w_coupling == 0.3  # COMPOSABLE bottom


def test_from_ranking_rejects_non_permutations():
    with pytest.raises(ValueError):
        WeightProfile.from_ranking(
            (Generator.SIMPLE, Generator.SIMPLE, Generator.SECURE)
        )  # type: ignore[arg-type]


# ---------------------------------------------------------------------- #
# Priority back-compat                                                    #
# ---------------------------------------------------------------------- #


def test_priority_top_generator():
    assert Priority.SIMPLE.top_generator() == Generator.SIMPLE
    assert Priority.COMPOSABLE.top_generator() == Generator.COMPOSABLE
    assert Priority.SECURE.top_generator() == Generator.SECURE


def test_priority_has_no_balanced_mode():
    """``BALANCED`` was the legacy escape hatch — gone now."""
    assert "BALANCED" not in {p.name for p in Priority}
    assert {p.value for p in Priority} == {"simple", "composable", "secure"}


def test_weight_profile_from_priority_covers_all_priorities():
    """Every Priority has a corresponding WeightProfile entry."""
    for p in Priority:
        profile = WeightProfile.from_priority(p)
        assert isinstance(profile, WeightProfile)
        assert profile is WEIGHT_PROFILES[p]
