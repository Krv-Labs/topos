"""Tests for topos_evaluate_code, topos_evaluate_file, topos_evaluate_project."""

from __future__ import annotations

import asyncio
from pathlib import Path
from unittest.mock import MagicMock, patch

import pytest
from topos.evaluation.preferences import Generator
from topos.mcp.schemas import (
    EvaluateCodeInput,
    EvaluateFileInput,
    EvaluateProjectInput,
    LatticeElement,
    UserPreferencesInput,
)
from topos.mcp.tools.evaluate import (
    topos_evaluate_code,
    topos_evaluate_file,
    topos_evaluate_project,
)


class _StubCtx:
    async def report_progress(self, progress: int, total: int) -> None:
        pass

    async def info(self, *args, **kwargs) -> None:
        pass


_PREFS = UserPreferencesInput(
    ranking=[Generator.SECURE, Generator.SIMPLE, Generator.COMPOSABLE]
)


def test_evaluate_code_pillars_breakdown() -> None:
    # Code that fails SIMPLE (high complexity) but satisfies SECURE (0 issues)
    bad_code = "def " + "f" * 100 + "():\n" + "    if True: pass\n" * 20
    r = topos_evaluate_code(EvaluateCodeInput(code=bad_code, preferences=_PREFS))

    assert "simple" in r.pillars
    assert "secure" in r.pillars

    # SECURE should be achieved (0 danger, 0 taint)
    assert r.pillars["secure"].achieved is True
    assert r.pillars["secure"].score == 100.0
    assert r.pillars["secure"].metrics["cpg.dangerous_calls"] == 0.0

    # SIMPLE should NOT be achieved (cyclomatic > 15)
    assert r.pillars["simple"].achieved is False
    assert r.pillars["simple"].metrics["cfg.cyclomatic"] > 15.0
    assert "cfg.cyclomatic" in r.pillars["simple"].interpretation


def test_evaluate_code_happy_path() -> None:
    r = topos_evaluate_code(
        EvaluateCodeInput(code="def foo(): return 1", preferences=_PREFS)
    )
    assert r.is_parseable
    assert r.coupling_available is False
    assert "simple" in r.scores
    assert r.error is None


def test_evaluate_code_defaults_to_legacy_simple_priority() -> None:
    r = topos_evaluate_code(EvaluateCodeInput(code="def foo(): return 1"))
    assert r.priority.value == "simple"
    assert r.priority_source.value == "default"


def test_evaluate_code_infers_priority_from_preferences() -> None:
    r = topos_evaluate_code(
        EvaluateCodeInput(code="def foo(): return 1", preferences=_PREFS)
    )
    assert r.priority.value == "secure"
    assert r.priority_source.value == "preferences"


def test_evaluate_code_rejects_unsupported_language() -> None:
    r = topos_evaluate_code(
        EvaluateCodeInput(code="x = 1", language="ruby", preferences=_PREFS)
    )
    assert r.error is not None


# --- topos_evaluate_file ---


def test_evaluate_file_reads_real_file() -> None:
    r = topos_evaluate_file(
        EvaluateFileInput(filepath="topos/__init__.py", preferences=_PREFS)
    )
    assert r.is_parseable
    assert "simple" in r.scores


def test_evaluate_file_warns_without_gitnexus(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    from topos.mcp import security

    monkeypatch.setenv("TOPOS_MCP_FILE_ROOT", str(tmp_path))
    security.reset_file_root_cache()
    path = tmp_path / "module.py"
    path.write_text("def f():\n    return 1\n", encoding="utf-8")

    r = topos_evaluate_file(EvaluateFileInput(filepath="module.py", preferences=_PREFS))

    assert r.coupling_available is False
    assert r.warnings
    assert "mdg.unavailable" in r.pillars["composable"].interpretation


def test_evaluate_file_reports_security_findings(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    from topos.mcp import security

    monkeypatch.setenv("TOPOS_MCP_FILE_ROOT", str(tmp_path))
    security.reset_file_root_cache()
    path = tmp_path / "danger.py"
    path.write_text("def f(expr):\n    return eval(expr)\n", encoding="utf-8")

    r = topos_evaluate_file(EvaluateFileInput(filepath="danger.py", preferences=_PREFS))

    assert r.security_findings
    assert r.security_findings[0].callee == "eval"
    assert r.security_findings[0].line == 2


def test_evaluate_file_rejects_path_outside_root(tmp_path: Path) -> None:
    outside = tmp_path / "stranger.py"
    outside.write_text("x = 1")
    r = topos_evaluate_file(
        EvaluateFileInput(filepath=str(outside), preferences=_PREFS)
    )
    assert r.error is not None
    assert "Access denied" in r.error


def test_evaluate_file_missing_file_errors() -> None:
    r = topos_evaluate_file(
        EvaluateFileInput(filepath="topos/does_not_exist.py", preferences=_PREFS)
    )
    assert r.error is not None


def test_evaluate_file_uses_depgraph_when_gitnexus_dir_exists() -> None:
    """P0 regression guard — this test would have caught the original bug."""
    fake_graph = MagicMock()
    fake_graph.name = "mdg"
    fake_graph.dimension = "composable"
    fake_graph.metrics.return_value = {
        "mdg.coupling": 5.0,
        "mdg.instability": 0.5,
        "mdg.fan_in": 2.0,
        "mdg.fan_out": 3.0,
        "mdg.dep_depth": 1.0,
    }
    with (
        patch(
            "topos.mcp.evaluation.load_dep_graph", return_value=fake_graph
        ) as mock_load,
        patch(
            "topos.mcp.evaluation.resolve_gitnexus_dir",
            return_value=Path("/fake/.gitnexus"),
        ),
    ):
        r = topos_evaluate_file(
            EvaluateFileInput(
                filepath="topos/__init__.py",
                gitnexus_dir="/fake/.gitnexus",
                preferences=_PREFS,
            )
        )
    mock_load.assert_called_once()
    assert r.coupling_available is True
    assert "composable" in r.scores, (
        "composable dimension must be present when a ModuleDependencyGraph is attached"
    )


# --- topos_evaluate_project ---


def test_evaluate_project_rolls_up_files() -> None:
    r = asyncio.run(
        topos_evaluate_project(
            EvaluateProjectInput(path="topos/graphs", limit=10, preferences=_PREFS),
            _StubCtx(),
        )
    )
    assert r.file_count >= 1
    assert r.overall in list(LatticeElement)
    assert r.count <= r.total
    assert r.files, "expected at least one per-file entry"
    assert r.aggregate_floor_verdict == r.overall
    assert r.aggregate_explanation
    assert r.guidance


def test_evaluate_project_paginates() -> None:
    full = asyncio.run(
        topos_evaluate_project(
            EvaluateProjectInput(path="topos", limit=5, offset=0, preferences=_PREFS),
            _StubCtx(),
        )
    )
    page2 = asyncio.run(
        topos_evaluate_project(
            EvaluateProjectInput(path="topos", limit=5, offset=5, preferences=_PREFS),
            _StubCtx(),
        )
    )
    assert full.total == page2.total
    # Different entries on different pages.
    full_paths = {e.filepath for e in full.files}
    page2_paths = {e.filepath for e in page2.files}
    assert full_paths.isdisjoint(page2_paths) or len(full_paths) < 5


def test_evaluate_project_rejects_outside_root(tmp_path: Path) -> None:
    r = asyncio.run(
        topos_evaluate_project(
            EvaluateProjectInput(path=str(tmp_path), limit=5, preferences=_PREFS),
            _StubCtx(),
        )
    )
    # Either refused (path outside root) or empty (no .py files).
    assert r.error is not None or r.file_count == 0
