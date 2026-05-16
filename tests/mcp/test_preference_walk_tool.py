"""Tests for the ``topos_preference_walk`` MCP tool."""

from __future__ import annotations

from topos.evaluation.preferences import Generator
from topos.mcp.schemas import (
    LatticeElement,
    PreferenceWalkInput,
)
from topos.mcp.tools.preferences import (
    render_preference_walk_md,
    topos_preference_walk,
)


def test_default_walk_starts_at_ideal_with_fallback_below():
    result = topos_preference_walk(
        PreferenceWalkInput(
            ranking=[Generator.SIMPLE, Generator.COMPOSABLE, Generator.SECURE]
        )
    )
    assert result.aspirational_target == LatticeElement.IDEAL
    # Top-2 meet for this ranking is SIMPLE_COMPOSABLE.
    assert result.fallback_target == LatticeElement.SIMPLE_COMPOSABLE
    # Walk starts at IDEAL and has the fallback as its second step.
    assert result.walk[0].verdict == LatticeElement.IDEAL
    assert result.walk[1].verdict == LatticeElement.SIMPLE_COMPOSABLE
    assert result.walk[1].verdict == result.fallback_target


def test_walk_truncates_above_current():
    result = topos_preference_walk(
        PreferenceWalkInput(
            ranking=[Generator.COMPOSABLE, Generator.SECURE, Generator.SIMPLE],
            current=LatticeElement.SECURE,
        )
    )
    # With this ranking SECURE has score 2; everything in the walk must
    # outrank it.  The smallest such verdict is SIMPLE_SECURE (score 3).
    assert result.next_step == LatticeElement.SIMPLE_SECURE
    # Current is reflected back in the result.
    assert result.current == LatticeElement.SECURE
    # Progress is fractional (SECURE = 2 / IDEAL = 7).
    assert 0.2 < result.progress < 0.3


def test_walk_empty_at_ideal():
    result = topos_preference_walk(
        PreferenceWalkInput(
            ranking=[Generator.SECURE, Generator.SIMPLE, Generator.COMPOSABLE],
            current=LatticeElement.IDEAL,
        )
    )
    assert result.next_step is None
    assert result.walk == []
    assert result.progress == 1.0


def test_steps_annotated_with_satisfied_generators():
    result = topos_preference_walk(
        PreferenceWalkInput(
            ranking=[Generator.SIMPLE, Generator.COMPOSABLE, Generator.SECURE]
        )
    )
    # Find the SIMPLE_SECURE step — should report exactly those two generators.
    step = next(s for s in result.walk if s.verdict == LatticeElement.SIMPLE_SECURE)
    assert set(step.generators_satisfied) == {Generator.SIMPLE, Generator.SECURE}


def test_induced_order_is_complete_and_descending():
    result = topos_preference_walk(
        PreferenceWalkInput(
            ranking=[Generator.SIMPLE, Generator.COMPOSABLE, Generator.SECURE]
        )
    )
    # All 8 elements appear.
    verdicts = [s.verdict for s in result.induced_order]
    assert len(verdicts) == 8
    assert set(verdicts) == set(LatticeElement)
    # Scores monotonically non-increasing.
    scores = [s.preference_score for s in result.induced_order]
    assert scores == sorted(scores, reverse=True)


def test_explicit_target_overrides_ideal():
    result = topos_preference_walk(
        PreferenceWalkInput(
            ranking=[Generator.SIMPLE, Generator.COMPOSABLE, Generator.SECURE],
            target=LatticeElement.SIMPLE_COMPOSABLE,
        )
    )
    assert result.aspirational_target == LatticeElement.SIMPLE_COMPOSABLE
    # IDEAL excluded — outranks the explicit target.
    walk_verdicts = [s.verdict for s in result.walk]
    assert LatticeElement.IDEAL not in walk_verdicts
    assert walk_verdicts[0] == LatticeElement.SIMPLE_COMPOSABLE


def test_markdown_render_includes_all_sections():
    result = topos_preference_walk(
        PreferenceWalkInput(
            ranking=[Generator.SECURE, Generator.SIMPLE, Generator.COMPOSABLE],
            current=LatticeElement.SIMPLE,
        )
    )
    md = render_preference_walk_md(result)
    assert "Preference Walk" in md
    assert "secure ≻ simple ≻ composable" in md
    assert "Aspirational target" in md
    assert "Fallback target" in md
    assert "Walk" in md
    assert "Full induced order" in md
