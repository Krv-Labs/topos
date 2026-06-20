"""Tests for topos_assess_improvement — especially the anti-gaming guardrail."""

from __future__ import annotations

from pathlib import Path
from unittest.mock import patch

import pytest
from topos.evaluation.preferences import Generator
from topos.mcp.schemas import (
    AssessImprovementInput,
    AssessmentResult,
    AssessmentStatus,
    UserPreferencesInput,
)
from topos.mcp.tools.assess import topos_assess_improvement

_PREFS = UserPreferencesInput(
    ranking=[Generator.SECURE, Generator.SIMPLE, Generator.COMPOSABLE]
)


def _assess(tool_result) -> AssessmentResult:
    """Rebuild the AssessmentResult model from a tool's ToolResult channel."""
    return AssessmentResult.model_validate(tool_result.structured_content)


def _content_text(tool_result) -> str:
    """The markdown text the LLM sees (first content block)."""
    return tool_result.content[0].text


def test_assess_requires_current_or_filepath() -> None:
    with pytest.raises(ValueError, match="filepath.*current_code"):
        AssessImprovementInput(proposed_code="x = 1", preferences=_PREFS)


def test_assess_accepts_proposed_filepath(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    from topos.mcp import security

    monkeypatch.setenv("TOPOS_MCP_FILE_ROOT", str(tmp_path))
    security.reset_file_root_cache()
    current = tmp_path / "current.py"
    proposed = tmp_path / "proposed.py"
    current.write_text("def f(x):\n    return x + 1\n", encoding="utf-8")
    proposed.write_text("def f(x):\n    return x + 2\n", encoding="utf-8")

    r = _assess(
        topos_assess_improvement(
            AssessImprovementInput(
                filepath="current.py",
                proposed_filepath="proposed.py",
                preferences=_PREFS,
            )
        )
    )

    assert r.error is None
    assert r.structural_distance is not None


def test_assess_reports_security_findings() -> None:
    current = "def f(expr):\n    return eval(expr)\n"
    tr = topos_assess_improvement(
        AssessImprovementInput(
            current_code=current,
            proposed_code=current,
            preferences=_PREFS,
        )
    )
    # Content block is compact markdown, NOT serialized JSON; structured
    # channel carries the key field.
    text = _content_text(tr)
    assert not text.lstrip().startswith("{")
    assert "status" in tr.structured_content
    r = _assess(tr)

    assert r.current.security_findings
    finding = r.current.security_findings[0]
    assert finding.kind == "dangerous_call"
    assert finding.line == 2
    assert finding.callee == "eval"


def test_assess_applies_allowlist_to_nested_evaluations(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    from topos.mcp import security

    monkeypatch.setenv("TOPOS_MCP_FILE_ROOT", str(tmp_path))
    security.reset_file_root_cache()
    current = tmp_path / "current.py"
    current.write_text("def f(expr):\n    return eval(expr)\n", encoding="utf-8")
    (tmp_path / ".topos.toml").write_text(
        '[[secure.allow]]\npattern = "eval"\nreason = "trusted REPL"\n',
        encoding="utf-8",
    )

    r = _assess(
        topos_assess_improvement(
            AssessImprovementInput(
                filepath="current.py",
                proposed_code=current.read_text(encoding="utf-8"),
                preferences=_PREFS,
            )
        )
    )

    assert r.current.secure_raw is False
    assert r.current.secure_adjusted is True
    assert r.current.security_findings == []
    assert r.current.acknowledged_risks[0].reason == "trusted REPL"
    assert r.proposed.secure_raw is False
    assert r.proposed.secure_adjusted is True
    assert r.proposed.security_findings == []
    assert r.proposed.acknowledged_risks[0].callee == "eval"


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
    r = _assess(
        topos_assess_improvement(
            AssessImprovementInput(
                current_code=current, proposed_code=proposed, preferences=_PREFS
            )
        )
    )
    # Mechanics: distance is computed, deltas reported, status classified.
    assert r.error is None
    assert r.structural_distance is not None
    assert r.structural_distance > 0.1
    assert "simple" in r.score_deltas
    # Status must be one of the valid enum members (any movement is fine).
    assert r.status in set(AssessmentStatus)
    assert r.agent_contract is not None
    assert r.agent_contract.verification_gates


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
        r = _assess(
            topos_assess_improvement(
                AssessImprovementInput(
                    current_code=code, proposed_code=code, preferences=_PREFS
                )
            )
        )
    assert r.structural_distance is not None
    assert r.structural_distance < 0.02
    assert r.status == AssessmentStatus.SUSPICIOUS_NO_STRUCTURAL_CHANGE
    assert r.suspicion_reason is not None
    assert "barely changed" in r.suspicion_reason
    assert r.agent_contract is not None
    assert "suspicious_no_structural_change" in r.agent_contract.blocked_by
    assert "metric_gaming_risk" in r.agent_contract.risk_flags


_SIMPLE_FN = "def handle(x):\n    return x + 1\n"
_BRANCHY_FN = (
    "def handle(x):\n"
    "    if x > 0:\n"
    "        if x > 10:\n"
    "            return 'big'\n"
    "        return 'pos'\n"
    "    elif x < 0:\n"
    "        return 'neg'\n"
    "    return 'zero'\n"
)


def _force_regression_score():
    """Patch the classifier so the *proposed* eval scores worse.

    Decouples the regression-status trigger from the lattice scoring model's
    quirks (added branching can score as an improvement under SIMPLE), so the
    test exercises the additive regression-diff path on a true regression.
    """
    import topos.mcp.evaluation as ev_mod
    from topos.core.omega import EvaluationValue

    original = ev_mod.classify_morphism

    # In current_code mode the baseline uses classify_code_string, so only the
    # *proposed* code flows through classify_morphism — force its lattice down.
    def fake_classify(morph, priority, dep_graph=None):
        result = original(morph, priority, dep_graph)
        result.lattice_element = EvaluationValue.SLOP
        result.scores["simple"] = result.scores.get("simple", 0.5) - 0.2
        return result

    return fake_classify


def test_assess_regression_emits_function_scoped_diff() -> None:
    """A regression yields a pinpoint diff of the worst function + its delta."""
    fake = _force_regression_score()
    with patch("topos.mcp.tools.assess.classify_morphism", side_effect=fake):
        tr = topos_assess_improvement(
            AssessImprovementInput(
                current_code=_SIMPLE_FN, proposed_code=_BRANCHY_FN, preferences=_PREFS
            )
        )
    r = _assess(tr)
    assert r.status in {
        AssessmentStatus.REGRESSION,
        AssessmentStatus.REGRESSION_SCORE,
        AssessmentStatus.SUSPICIOUS_NO_STRUCTURAL_CHANGE,
    }
    assert r.regression_diff is not None
    # Names the regressing function and the complexity movement.
    assert "handle" in r.regression_diff
    assert "cyclomatic complexity" in r.regression_diff
    assert "+" in r.regression_diff  # added lines / positive delta
    # Markdown carries a fenced diff block.
    text = _content_text(tr)
    assert "## Regression diff" in text
    assert "```diff" in text


def test_assess_improvement_has_no_regression_diff() -> None:
    """A non-regression verdict leaves regression_diff None."""
    tr = topos_assess_improvement(
        AssessImprovementInput(
            current_code=_SIMPLE_FN, proposed_code=_BRANCHY_FN, preferences=_PREFS
        )
    )
    r = _assess(tr)
    # Unpatched, added branching scores as an improvement here.
    assert r.status == AssessmentStatus.IMPROVEMENT
    assert r.regression_diff is None
    assert "## Regression diff" not in _content_text(tr)


def test_function_complexities_non_ascii_source() -> None:
    # Regression: UAST byte spans vs. code-point str indexing. A non-ASCII char
    # before the def used to shift both the extracted name and body slice.
    from topos.mcp.tools.assess import _function_complexities

    src = '"""Docstring → with — non-ascii 🎯."""\ndef handle(x):\n    return x\n'
    fns = _function_complexities(src, "python")
    assert "handle" in fns
    assert all(name.isidentifier() for name in fns), fns
    _complexity, body_lines = fns["handle"]
    body = "\n".join(body_lines)
    # Body round-trips to the real function, not a shifted fragment.
    assert body.startswith("def handle(x):")
    assert "return x" in body


def test_assess_filepath_path_validation() -> None:
    import tempfile

    with tempfile.NamedTemporaryFile(suffix=".py", delete=False) as f:
        f.write(b"def x(): pass")
        bad = f.name
    r = _assess(
        topos_assess_improvement(
            AssessImprovementInput(
                filepath=bad, proposed_code="def x(): return 1", preferences=_PREFS
            )
        )
    )
    assert r.error is not None  # outside repo root
