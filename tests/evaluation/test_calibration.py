"""Tests for centralized policy calibration in calibration.py."""

from __future__ import annotations

from topos.evaluation.policies.calibration import (
    CLONE,
    COMPOSABLE,
    COVERAGE,
    SCORE_FLOORS,
    SECURE,
    SIMPLE,
)
from topos.evaluation.policies.composable import score_coupling
from topos.evaluation.policies.secure import score_secure
from topos.evaluation.policies.simple import score_simple
from topos.evaluation.preferences import Generator


def test_score_floors_cover_every_generator():
    for g in Generator:
        assert g in SCORE_FLOORS
        assert 0.0 < SCORE_FLOORS[g] <= 1.0


def test_score_floors_match_pypi_calibration():
    assert SCORE_FLOORS[Generator.SIMPLE] == 0.40
    assert SCORE_FLOORS[Generator.COMPOSABLE] == 0.80
    assert SCORE_FLOORS[Generator.SECURE] == 1.00


def test_secure_gates_are_zero_tolerance():
    assert SECURE.max_dangerous_calls == 0.0
    assert SECURE.max_taint_flows == 0.0


def test_simple_entropy_band_is_ordered():
    assert 0.0 <= SIMPLE.min_entropy < SIMPLE.entropy_ideal < SIMPLE.max_entropy <= 1.0


def test_composable_instability_band_is_ordered():
    assert 0.0 <= COMPOSABLE.instability_low < COMPOSABLE.instability_high <= 1.0


def test_coverage_and_clone_defaults_are_sane():
    assert 0.0 < COVERAGE.declaration_recall <= 1.0
    assert COVERAGE.strong_offset > 0.0
    assert 0.0 < COVERAGE.partial_factor < 1.0
    assert 0.0 < CLONE.max_normalized_distance < 1.0


def test_simple_achieved_boundary_uses_calibration():
    # Pass all gates
    assert (
        score_simple(
            SIMPLE.max_cyclomatic - 5,
            SIMPLE.entropy_ideal,
            SIMPLE.max_function_complexity - 5,
        ).achieved
        is True
    )

    # Fail cyclomatic
    assert (
        score_simple(
            SIMPLE.max_cyclomatic + 1,
            SIMPLE.entropy_ideal,
            SIMPLE.max_function_complexity - 5,
        ).achieved
        is False
    )

    # Fail entropy (high)
    assert (
        score_simple(
            SIMPLE.max_cyclomatic - 5,
            SIMPLE.max_entropy + 0.1,
            SIMPLE.max_function_complexity - 5,
        ).achieved
        is False
    )

    # Fail max function complexity
    assert (
        score_simple(
            SIMPLE.max_cyclomatic - 5,
            SIMPLE.entropy_ideal,
            SIMPLE.max_function_complexity + 1,
        ).achieved
        is False
    )


def test_secure_achieved_boundary_uses_calibration():
    assert score_secure(dangerous_calls=0, taint_flows=0).achieved is True
    assert (
        score_secure(
            dangerous_calls=SECURE.max_dangerous_calls + 1, taint_flows=0
        ).achieved
        is False
    )
    assert (
        score_secure(dangerous_calls=0, taint_flows=SECURE.max_taint_flows + 1).achieved
        is False
    )


def test_composable_achieved_boundary_uses_calibration():
    mid_instability = (COMPOSABLE.instability_low + COMPOSABLE.instability_high) / 2
    assert (
        score_coupling(
            instability=mid_instability,
            fan_in=COMPOSABLE.max_fan_in - 5,
            fan_out=COMPOSABLE.max_fan_out - 5,
        ).achieved
        is True
    )

    assert (
        score_coupling(
            instability=COMPOSABLE.instability_low - 0.1,
            fan_in=COMPOSABLE.max_fan_in - 5,
            fan_out=COMPOSABLE.max_fan_out - 5,
        ).achieved
        is False
    )

    assert (
        score_coupling(
            instability=mid_instability,
            fan_in=COMPOSABLE.max_fan_in + 1,
            fan_out=COMPOSABLE.max_fan_out - 5,
        ).achieved
        is False
    )

    assert (
        score_coupling(
            instability=mid_instability,
            fan_in=COMPOSABLE.max_fan_in - 5,
            fan_out=COMPOSABLE.max_fan_out + 1,
        ).achieved
        is False
    )
