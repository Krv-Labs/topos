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
    from topos.mcp import security

    monkeypatch.setenv("TOPOS_MCP_FILE_ROOT", str(tmp_path))
    security.reset_file_root_cache()
    (tmp_path / "module.py").write_text(_COMPLEX_FN, encoding="utf-8")

    result = _eval(topos_evaluate_file(EvaluateFileInput(filepath="module.py")))

    assert result.refactor_targets == []


def test_evaluate_file_returns_complex_function_target(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    from topos.mcp import security

    monkeypatch.setenv("TOPOS_MCP_FILE_ROOT", str(tmp_path))
    security.reset_file_root_cache()
    (tmp_path / "module.py").write_text(_COMPLEX_FN, encoding="utf-8")

    tr = topos_evaluate_file(
        EvaluateFileInput(filepath="module.py", include_refactor_targets=True)
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
    assert "topos_assess_worktree_change" in target.verify_with
    assert result.agent_contract is not None
    assert result.agent_contract.next_tool == "topos_assess_worktree_change"
    assert "## Refactor Targets" in _content_text(tr)


def test_evaluate_file_returns_security_target_first_when_preferred(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    from topos.mcp import security
    from topos.mcp.schemas import UserPreferencesInput

    monkeypatch.setenv("TOPOS_MCP_FILE_ROOT", str(tmp_path))
    security.reset_file_root_cache()
    (tmp_path / "danger.py").write_text(
        _COMPLEX_FN + "\ndef danger(expr):\n    return eval(expr)\n",
        encoding="utf-8",
    )

    result = _eval(
        topos_evaluate_file(
            EvaluateFileInput(
                filepath="danger.py",
                include_refactor_targets=True,
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
    from topos.mcp import security

    monkeypatch.setenv("TOPOS_MCP_FILE_ROOT", str(tmp_path))
    security.reset_file_root_cache()
    (tmp_path / "module.py").write_text("def f():\n    return 1\n", encoding="utf-8")

    result = _eval(
        topos_evaluate_file(
            EvaluateFileInput(filepath="module.py", include_refactor_targets=True)
        )
    )

    assert result.agent_contract is not None
    assert "missing_gitnexus_dir" in result.agent_contract.blocked_by


def test_evaluate_file_refactor_targets_cap_output(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    from topos.mcp import security

    monkeypatch.setenv("TOPOS_MCP_FILE_ROOT", str(tmp_path))
    security.reset_file_root_cache()
    code = "\n\n".join(
        f"def f{i}(x):\n"
        + "".join(f"    if x == {j}:\n        return {j}\n" for j in range(12))
        for i in range(4)
    )
    (tmp_path / "many.py").write_text(code, encoding="utf-8")

    result = _eval(
        topos_evaluate_file(
            EvaluateFileInput(
                filepath="many.py",
                include_refactor_targets=True,
                max_refactor_targets=2,
            )
        )
    )

    assert len(result.refactor_targets) == 2
    assert result.raw_metrics == {}


def test_refactor_targets_no_extra_tool_registered() -> None:
    from topos.mcp import tools  # noqa: F401
    from topos.mcp.server import _get_mcp

    async def _names() -> set[str]:
        return {tool.name for tool in await _get_mcp().list_tools()}

    assert "topos_refactor_targets" not in asyncio.run(_names())
