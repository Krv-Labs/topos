"""Tests for evaluate-file refactor targets."""

from __future__ import annotations

import asyncio
from pathlib import Path

import pytest
from topos.evaluation.preferences import Generator
from topos.mcp.schemas import EvaluateFileInput, EvaluationResult
from topos.mcp.tools.evaluate.core import topos_evaluate_file


def _eval(tool_result) -> EvaluationResult:
    return EvaluationResult.model_validate(tool_result.structured_content)


def _content_text(tool_result) -> str:
    return tool_result.content[0].text


def _use_root(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    from topos.mcp import security

    monkeypatch.setenv("TOPOS_MCP_FILE_ROOT", str(tmp_path))
    security.reset_file_root_cache()


_PREF_SECURE_FIRST = [
    Generator.SECURE,
    Generator.SIMPLE,
    Generator.COMPOSABLE,
]

_COMPLEX_FN = "def big(x):\n" + "".join(
    f"    if x == {i}:\n        return {i}\n" for i in range(12)
)


def test_evaluate_file_refactor_targets_default_off(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    _use_root(tmp_path, monkeypatch)
    (tmp_path / "module.py").write_text(_COMPLEX_FN, encoding="utf-8")

    result = _eval(topos_evaluate_file(EvaluateFileInput(filepath="module.py")))

    assert result.refactor_targets == []


def test_default_off_hints_refactor_targets_when_below_ideal(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """Agents discover the option from the contract, not the schema."""
    _use_root(tmp_path, monkeypatch)
    (tmp_path / "module.py").write_text(_COMPLEX_FN, encoding="utf-8")

    result = _eval(topos_evaluate_file(EvaluateFileInput(filepath="module.py")))

    assert result.agent_contract is not None
    assert any(
        "refactor_targets=5" in action for action in result.agent_contract.next_actions
    )


def test_requested_targets_do_not_hint(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    _use_root(tmp_path, monkeypatch)
    (tmp_path / "module.py").write_text(_COMPLEX_FN, encoding="utf-8")

    result = _eval(
        topos_evaluate_file(EvaluateFileInput(filepath="module.py", refactor_targets=5))
    )

    assert result.agent_contract is not None
    assert not any(
        "refactor_targets=5" in action for action in result.agent_contract.next_actions
    )


def test_evaluate_file_returns_complex_function_target(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    _use_root(tmp_path, monkeypatch)
    (tmp_path / "module.py").write_text(_COMPLEX_FN, encoding="utf-8")

    tr = topos_evaluate_file(
        EvaluateFileInput(filepath="module.py", refactor_targets=5)
    )
    result = _eval(tr)

    assert result.refactor_targets
    target = result.refactor_targets[0]
    assert target.kind == "function"
    assert target.symbol == "big"
    assert target.metric == "ast.max_function_complexity"
    assert target.line_start == 1
    assert target.line_end is not None and target.line_end > target.line_start
    assert "extract_helper" in target.recommended_operations
    assert result.agent_contract is not None
    assert result.agent_contract.next_tool == "topos_assess_worktree_change"
    assert "## Refactor Targets" in _content_text(tr)
    # Regression for the post-hoc contract overwrite: the setup blocker and
    # its remedy must survive alongside the edit step.
    assert "missing_gitnexus_dir" in result.agent_contract.blocked_by
    actions = result.agent_contract.next_actions
    assert any(action.startswith("edit target rt_") for action in actions)
    assert any("topos_generate_depgraph" in action for action in actions)


def test_evaluate_file_returns_security_target_first_when_preferred(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    from topos.mcp.schemas import UserPreferencesInput

    _use_root(tmp_path, monkeypatch)
    (tmp_path / "danger.py").write_text(
        _COMPLEX_FN + "\ndef danger(expr):\n    return eval(expr)\n",
        encoding="utf-8",
    )

    result = _eval(
        topos_evaluate_file(
            EvaluateFileInput(
                filepath="danger.py",
                refactor_targets=5,
                preferences=UserPreferencesInput(ranking=_PREF_SECURE_FIRST),
            )
        )
    )

    assert result.refactor_targets
    target = result.refactor_targets[0]
    assert target.kind == "security_call"
    assert target.failing_generators == ["secure"]
    assert target.metric == "eval"
    assert "replace_dynamic_execution" in target.recommended_operations


def test_evaluate_file_refactor_targets_preserve_gitnexus_blocker(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    _use_root(tmp_path, monkeypatch)
    (tmp_path / "module.py").write_text("def f():\n    return 1\n", encoding="utf-8")

    result = _eval(
        topos_evaluate_file(EvaluateFileInput(filepath="module.py", refactor_targets=5))
    )

    assert result.agent_contract is not None
    assert "missing_gitnexus_dir" in result.agent_contract.blocked_by


def test_evaluate_file_refactor_targets_cap_output(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    _use_root(tmp_path, monkeypatch)
    code = "\n\n".join(
        f"def f{i}(x):\n"
        + "".join(f"    if x == {j}:\n        return {j}\n" for j in range(12))
        for i in range(4)
    )
    (tmp_path / "many.py").write_text(code, encoding="utf-8")

    result = _eval(
        topos_evaluate_file(EvaluateFileInput(filepath="many.py", refactor_targets=2))
    )

    assert len(result.refactor_targets) == 2
    assert result.raw_metrics == {}


def test_security_target_ranks_by_real_zero_threshold() -> None:
    """Zero-threshold security targets must rank by real excess (falsy-zero fix)."""
    from topos.evaluation.characteristic_morphism import ClassificationResult
    from topos.mcp.refactor_targets import build_refactor_targets
    from topos.mcp.schemas import SecurityFinding

    result = ClassificationResult(is_parseable=True)
    finding = SecurityFinding(
        kind="dangerous_call", line=3, snippet="eval(x)", callee="eval"
    )
    targets = build_refactor_targets(
        filepath="mod.py",
        result=result,
        security_findings=[finding],
        locations={},
        ranking=_PREF_SECURE_FIRST,
    )

    assert targets
    assert targets[0].kind == "security_call"
    # threshold=0.0 must not be treated as missing: excess is |1.0 - 0.0|.
    assert targets[0].threshold == 0.0
    assert targets[0].current_value == 1.0


def test_entrypoint_exempt_gates_produce_no_module_targets() -> None:
    """Targets never contradict the score: exempt gates are not targets."""
    from topos.evaluation.characteristic_morphism import ClassificationResult
    from topos.mcp.refactor_targets import build_refactor_targets

    result = ClassificationResult(
        is_parseable=True,
        raw_metrics={"ast.entropy": 0.05, "mdg.instability": 1.0, "mdg.fan_in": 0.0},
        is_entrypoint_module=True,
    )
    targets = build_refactor_targets(
        filepath="pkg/__init__.py",
        result=result,
        security_findings=[],
        locations={},
    )

    assert targets == []


def test_low_entropy_gets_direction_specific_operations() -> None:
    from topos.evaluation.characteristic_morphism import ClassificationResult
    from topos.mcp.refactor_targets import build_refactor_targets

    result = ClassificationResult(
        is_parseable=True,
        raw_metrics={"ast.entropy": 0.05},
    )
    targets = build_refactor_targets(
        filepath="mod.py",
        result=result,
        security_findings=[],
        locations={},
    )

    assert len(targets) == 1
    assert targets[0].recommended_operations == ["consolidate_boilerplate"]


def test_refactor_targets_no_extra_tool_registered() -> None:
    from topos.mcp import tools  # noqa: F401
    from topos.mcp.server import _get_mcp

    async def _names() -> set[str]:
        return {tool.name for tool in await _get_mcp().list_tools()}

    assert "topos_refactor_targets" not in asyncio.run(_names())
