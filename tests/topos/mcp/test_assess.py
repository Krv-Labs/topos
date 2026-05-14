"""Tests for topos_assess_improvement — especially the anti-gaming guardrail."""

from __future__ import annotations

from unittest.mock import patch

from topos.mcp.schemas import AssessImprovementInput, AssessmentStatus
from topos.mcp.tools.assess import topos_assess_improvement


def test_assess_requires_current_or_filepath() -> None:
    r = topos_assess_improvement(AssessImprovementInput(proposed_code="x = 1"))
    assert r.error is not None


def test_assess_emits_distance_and_deltas_on_real_change() -> None:
    """Any meaningful code change should produce nonzero AST distance and deltas."""
    current = (
        "def f(x):\n"
        "    if x > 0:\n"
        "        if x > 10:\n"
        "            if x > 100:\n"
        "                return 'huge'\n"
        "            return 'medium'\n"
        "        return 'small'\n"
        "    return 'zero'\n"
    )
    proposed = (
        "def f(x):\n"
        "    buckets = [(100, 'huge'), (10, 'medium'), (0, 'small')]\n"
        "    for threshold, label in buckets:\n"
        "        if x > threshold:\n"
        "            return label\n"
        "    return 'zero'\n"
    )
    r = topos_assess_improvement(
        AssessImprovementInput(current_code=current, proposed_code=proposed)
    )
    # Mechanics: distance is computed, deltas reported, status classified.
    assert r.error is None
    assert r.structural_distance is not None
    assert r.structural_distance > 0.1
    assert "simple" in r.score_deltas
    # Status must be one of the valid enum members (any movement is fine).
    assert r.status in set(AssessmentStatus)


def test_assess_flags_suspicious_no_structural_change() -> None:
    """Anti-gaming guardrail: near-zero AST distance + improved score = flag."""

    # Patch the classifier to return improving scores for the proposed code
    # without the tree actually changing meaningfully.

    import topos.mcp.evaluation as ev_mod

    original = ev_mod.classify_morphism

    call_count = {"n": 0}

    def fake_classify(morph, priority, dep_graph=None):
        call_count["n"] += 1
        result = original(morph, priority, dep_graph)
        if call_count["n"] == 2:
            # Boost the "proposed" scores without changing the morphism.
            # Only nudge SIMPLE — keep other dims so deltas don't show
            # spurious regressions on unchanged generators.
            result.scores["simple"] = result.scores.get("simple", 0.5) + 0.15
        return result

    code = "def f(x):\n    return x + 1\n"

    with (
        patch.object(ev_mod, "classify_morphism", side_effect=fake_classify),
        patch(
            "topos.mcp.tools.assess.classify_morphism",
            side_effect=fake_classify,
        ),
    ):
        r = topos_assess_improvement(
            AssessImprovementInput(current_code=code, proposed_code=code)
        )
    assert r.structural_distance is not None
    assert r.structural_distance < 0.02
    assert r.status == AssessmentStatus.SUSPICIOUS_NO_STRUCTURAL_CHANGE
    assert r.suspicion_reason is not None
    assert "barely changed" in r.suspicion_reason


def test_assess_filepath_path_validation() -> None:
    import tempfile

    with tempfile.NamedTemporaryFile(suffix=".py", delete=False) as f:
        f.write(b"def x(): pass")
        bad = f.name
    r = topos_assess_improvement(
        AssessImprovementInput(filepath=bad, proposed_code="def x(): return 1")
    )
    assert r.error is not None  # outside repo root
