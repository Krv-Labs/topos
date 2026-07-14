"""Tests for centralized policy calibration in calibration.py."""

from __future__ import annotations

import pytest
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


def test_simple_entrypoint_tolerates_low_entropy():
    decision = score_simple(
        cyclomatic=SIMPLE.max_cyclomatic - 5,
        entropy=SIMPLE.min_entropy - 0.05,
        max_function_complexity=SIMPLE.max_function_complexity - 5,
        is_entrypoint_module=True,
    )
    assert decision.achieved is True
    assert "entrypoint modules" in decision.interpretation["ast.entropy"]


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


def test_composable_entrypoint_tolerates_high_instability_when_fan_in_zero():
    decision = score_coupling(
        instability=1.0,
        fan_in=0.0,
        fan_out=2.0,
        is_entrypoint_module=True,
    )
    assert decision.achieved is True
    assert "entrypoint modules" in decision.interpretation["mdg.instability"]


def test_composable_high_instability_still_fails_without_entrypoint_flag():
    decision = score_coupling(
        instability=1.0,
        fan_in=0.0,
        fan_out=2.0,
    )
    assert decision.achieved is False


# ---------------------------------------------------------------------------
# Distance-from-Main-Sequence gate (issue #124) — abstractness present
# ---------------------------------------------------------------------------


def test_composable_orchestrator_passes_via_distance_without_entrypoint_flag():
    # I=1, A=0 (concrete, unstable orchestrator, e.g. main.rs) sits on the
    # main sequence (D=0) — should pass without needing is_entrypoint_module.
    decision = score_coupling(instability=1.0, abstractness=0.0, fan_in=0.0)
    assert decision.achieved is True
    assert "within tolerance" in decision.interpretation["mdg.main_sequence_distance"]
    # Informational only — mdg.instability itself is not gated here, but is
    # still surfaced so users see why a "too high" reading isn't a failure.
    assert "too high" in decision.interpretation["mdg.instability"]


def test_composable_stable_leaf_fails_distance_without_exemption():
    # I=0, A=0 (concrete, stable leaf, e.g. constants.rs) is Martin's "Zone
    # of Pain" — D=1, maximal distance — and fails without the leaf role.
    decision = score_coupling(instability=0.0, abstractness=0.0)
    assert decision.achieved is False


def test_composable_stable_leaf_passes_with_exemption():
    decision = score_coupling(
        instability=0.0, abstractness=0.0, is_stable_leaf_module=True
    )
    assert decision.achieved is True
    assert "tolerated" in decision.interpretation["mdg.main_sequence_distance"]


def test_composable_abstract_unstable_entrypoint_still_fails_distance():
    # I=1, A=1 -> D=1 is Martin's "Zone of Uselessness" (abstract types that
    # nothing depends on). Unlike the "Zone of Pain", it has no accepted
    # exception, so `is_entrypoint_module` deliberately grants NO distance
    # carve-out here (issue #157). The entrypoint exemption applies only to
    # the raw-instability gate; a concrete unstable entrypoint already passes
    # distance via geometry (see the orchestrator test above), so distance
    # mode needs no entrypoint exemption. This pins that intent against the
    # instability-mode docstring being copied back into the distance gate.
    decision = score_coupling(
        instability=1.0, abstractness=1.0, fan_in=0.0, is_entrypoint_module=True
    )
    assert decision.achieved is False


def test_composable_tangled_module_still_fails_with_abstractness():
    # I=0.9, A=0.9 -> D=0.8, well past the distance gate: neither role
    # exemption applies, so this must still fail.
    decision = score_coupling(instability=0.9, abstractness=0.9)
    assert decision.achieved is False


def test_composable_distance_boundary_uses_calibration():
    max_d = COMPOSABLE.main_sequence_distance_max
    # A + I - 1 = max_d exactly, at the boundary -> should pass (inclusive).
    instability = 1.0
    abstractness = max_d  # |A + I - 1| = |max_d + 1 - 1| = max_d
    decision = score_coupling(instability=instability, abstractness=abstractness)
    assert decision.achieved is True

    # Just past the boundary -> should fail.
    decision = score_coupling(instability=instability, abstractness=abstractness + 0.01)
    assert decision.achieved is False


def test_composable_without_abstractness_keeps_instability_gate_untouched():
    # No abstractness supplied -> mdg.instability remains the gated metric,
    # byte-identical to pre-issue-124 behavior (fallback path).
    decision = score_coupling(instability=1.0, fan_in=0.0, fan_out=2.0)
    assert decision.achieved is False
    assert "mdg.main_sequence_distance" not in decision.interpretation
    assert "mdg.instability" in decision.interpretation


def test_composable_zero_coupling_signal_falls_back_to_instability_gate():
    # A file with zero *measured* fan-in/fan-out gets instability=0.5 from
    # calculate_coupling's "no signal" fallback (see coupling.py), which
    # sits mid the optimal instability band. Pairing that fallback with the
    # common abstractness=0.0 case (no type declarations) would otherwise
    # land main_sequence_distance exactly on main_sequence_distance_max --
    # passing the hard gate at the boundary but scoring 0.0 on the quality
    # curve. Distance mode must not activate without a real coupling signal.
    decision = score_coupling(
        instability=0.5, abstractness=0.0, fan_in=0.0, fan_out=0.0
    )
    assert decision.achieved is True
    assert decision.score == 1.0
    assert "mdg.main_sequence_distance" not in decision.interpretation
    assert "mdg.instability" in decision.interpretation


# ---------------------------------------------------------------------------
# Entropy size-floor score shaping (issue #152)
# ---------------------------------------------------------------------------


def test_simple_entropy_below_floor_above_ideal_is_not_penalized():
    # A tiny (< entropy_size_floor_bytes) source whose entropy reads above
    # entropy_ideal is not a reliable "too dense" signal -- zlib's fixed
    # per-stream overhead dominates the ratio at this size. The entropy
    # quality must be 1.0 regardless of how far above ideal it reads.
    decision = score_simple(
        cyclomatic=0,
        entropy=SIMPLE.max_entropy,  # as high as the hard gate still allows
        max_function_complexity=0,
        source_size_bytes=50,
    )
    assert decision.interpretation["ast.entropy"]
    assert decision.score == 1.0


def test_simple_entropy_below_floor_below_ideal_still_penalized():
    # Below-ideal entropy (repetition/boilerplate) stays a reliable signal
    # at any size, so the size floor must not suppress that penalty.
    decision = score_simple(
        cyclomatic=1,
        entropy=SIMPLE.min_entropy,
        max_function_complexity=1,
        source_size_bytes=50,
    )
    assert decision.score < 1.0


def test_simple_entropy_above_floor_keeps_symmetric_penalty():
    # Once the source clears the size floor, behavior is byte-identical to
    # before issue #152: above-ideal entropy is penalized the same as
    # below-ideal entropy by the same distance.
    above = score_simple(
        cyclomatic=1,
        entropy=SIMPLE.entropy_ideal + 0.2,
        max_function_complexity=1,
        source_size_bytes=SIMPLE.entropy_size_floor_bytes,
    )
    below = score_simple(
        cyclomatic=1,
        entropy=SIMPLE.entropy_ideal - 0.2,
        max_function_complexity=1,
        source_size_bytes=SIMPLE.entropy_size_floor_bytes,
    )
    assert above.score == pytest.approx(below.score)


def test_simple_entropy_no_size_supplied_keeps_symmetric_penalty():
    # Callers that don't pass source_size_bytes (legacy call sites) keep
    # the pre-fix symmetric curve -- the floor is opt-in via the new arg.
    decision = score_simple(
        cyclomatic=1, entropy=SIMPLE.max_entropy, max_function_complexity=1
    )
    assert decision.score < 1.0


def test_simple_tiny_array_lookup_no_longer_scores_below_tiny_match():
    # Issue #152's exact repro: refactoring an 8-arm match into a 3-line
    # array lookup is strictly simpler (cyclomatic 8 -> 1) but used to
    # score *worse* on SIMPLE because its entropy read further above
    # entropy_ideal than the match's. Both are well under the entropy
    # size floor (~130 bytes each).
    match_arm = score_simple(
        cyclomatic=8,
        entropy=0.670,
        max_function_complexity=2,
        source_size_bytes=130,
    )
    array_lookup = score_simple(
        cyclomatic=1,
        entropy=0.738,
        max_function_complexity=1,
        source_size_bytes=130,
    )
    assert array_lookup.score >= match_arm.score
    assert array_lookup.achieved is True
