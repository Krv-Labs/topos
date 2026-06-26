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
    EvaluationResult,
    LatticeElement,
    ProjectEvaluationResult,
    UserPreferencesInput,
)
from topos.mcp.tools.evaluate.core import (
    topos_evaluate_code,
    topos_evaluate_file,
)
from topos.mcp.tools.evaluate.project import topos_evaluate_project


def _eval(tool_result) -> EvaluationResult:
    """Rebuild the EvaluationResult model from a tool's ToolResult channel."""
    return EvaluationResult.model_validate(tool_result.structured_content)


def _project(tool_result) -> ProjectEvaluationResult:
    """Rebuild the ProjectEvaluationResult model from a tool's ToolResult."""
    return ProjectEvaluationResult.model_validate(tool_result.structured_content)


def _content_text(tool_result) -> str:
    """The markdown text the LLM sees (first content block)."""
    return tool_result.content[0].text


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
    # verbose=True surfaces the full raw-metric floats in the structured channel.
    r = _eval(
        topos_evaluate_code(
            EvaluateCodeInput(code=bad_code, preferences=_PREFS, verbose=True)
        )
    )

    assert "simple" in r.pillars
    assert "secure" in r.pillars

    # SECURE should be achieved (0 danger, 0 taint)
    assert r.pillars["secure"].achieved is True
    assert r.pillars["secure"].score == 100.0
    # Metric/interpretation detail lives once in the flat maps, not per-pillar.
    assert r.raw_metrics["cpg.dangerous_calls"] == 0.0

    # SIMPLE should NOT be achieved (cyclomatic > 15)
    assert r.pillars["simple"].achieved is False
    assert r.raw_metrics["cfg.cyclomatic"] > 15.0
    assert "cfg.cyclomatic" in r.interpretation


def test_evaluate_default_gates_raw_metrics_from_both_channels() -> None:
    # High-complexity code so SIMPLE fails and has an interpretation entry.
    bad_code = "def f():\n" + "    if True: pass\n" * 20
    tr = topos_evaluate_code(EvaluateCodeInput(code=bad_code, preferences=_PREFS))
    text = _content_text(tr)
    r = _eval(tr)

    # Markdown omits the Raw Metrics table by default.
    assert "## Raw Metrics" not in text
    # Structured channel drops raw_metrics by default.
    assert r.raw_metrics == {}
    # ...but keeps the interpretation for the FAILING simple generator.
    assert "cfg.cyclomatic" in r.interpretation
    # ...and drops interpretation for satisfied generators (secure passed).
    assert not any(k.startswith("cpg.") for k in r.interpretation)


def test_evaluate_verbose_restores_full_detail() -> None:
    bad_code = "def f():\n" + "    if True: pass\n" * 20
    tr = topos_evaluate_code(
        EvaluateCodeInput(code=bad_code, preferences=_PREFS, verbose=True)
    )
    assert "## Raw Metrics" in _content_text(tr)
    r = _eval(tr)
    assert r.raw_metrics["cfg.cyclomatic"] > 15.0
    assert any(k.startswith("cpg.") for k in r.interpretation)


def test_evaluate_surfaces_actionable_suggestions() -> None:
    # High-complexity code so SIMPLE fails its gate.
    bad_code = "def f():\n" + "    if True: pass\n" * 20
    tr = topos_evaluate_code(EvaluateCodeInput(code=bad_code, preferences=_PREFS))
    r = _eval(tr)

    # Structured channel carries a fix-severity suggestion for the failing
    # simple generator.
    assert r.suggestions, "expected suggestions for failing SIMPLE code"
    simple_fixes = [
        s for s in r.suggestions if s.pillar == "simple" and s.severity == "fix"
    ]
    assert simple_fixes, r.suggestions
    assert r.agent_contract is not None
    assert r.agent_contract.next_tool == "topos_inspect_code"
    assert "topos_assess_improvement returns IMPROVEMENT or IMPROVEMENT_SCORE" in (
        r.agent_contract.verification_gates
    )

    # Markdown shows the checklist by default (not verbose-gated).
    text = _content_text(tr)
    assert "## Suggestions" in text
    assert "## Agent Contract" in text
    assert "- [ ] (simple)" in text


def test_evaluate_clean_code_has_no_suggestions() -> None:
    clean_code = (
        "def add(a, b):\n"
        "    result = a + b\n"
        "    return result\n\n\n"
        "def greet(name):\n"
        '    message = "hello " + name\n'
        "    return message\n"
    )
    tr = topos_evaluate_code(EvaluateCodeInput(code=clean_code, preferences=_PREFS))
    r = _eval(tr)
    assert r.suggestions == []
    assert r.agent_contract is not None
    assert r.agent_contract.next_actions
    assert "## Suggestions" not in _content_text(tr)


def test_evaluate_code_returns_markdown_content_and_structured() -> None:
    tr = topos_evaluate_code(
        EvaluateCodeInput(code="def foo(): return 1", preferences=_PREFS)
    )
    text = _content_text(tr)
    # Content block is compact markdown, NOT serialized JSON.
    assert not text.lstrip().startswith("{")
    assert text.lstrip().startswith("**Lattice:**")
    # Structured channel still carries the model for programmatic clients.
    assert tr.structured_content is not None
    assert "lattice_element" in tr.structured_content


def test_evaluate_code_happy_path() -> None:
    r = _eval(
        topos_evaluate_code(
            EvaluateCodeInput(code="def foo(): return 1", preferences=_PREFS)
        )
    )
    assert r.is_parseable
    assert r.coupling_available is False
    assert "simple" in r.scores
    assert r.error is None


def test_evaluate_code_defaults_to_legacy_simple_priority() -> None:
    r = _eval(topos_evaluate_code(EvaluateCodeInput(code="def foo(): return 1")))
    assert r.priority.value == "simple"
    assert r.priority_source.value == "default"


def test_evaluate_code_infers_priority_from_preferences() -> None:
    r = _eval(
        topos_evaluate_code(
            EvaluateCodeInput(code="def foo(): return 1", preferences=_PREFS)
        )
    )
    assert r.priority.value == "secure"
    assert r.priority_source.value == "preferences"


def test_evaluate_code_rejects_unsupported_language() -> None:
    r = _eval(
        topos_evaluate_code(
            EvaluateCodeInput(code="x = 1", language="ruby", preferences=_PREFS)
        )
    )
    assert r.error is not None


# --- topos_evaluate_file ---


def test_evaluate_file_reads_real_file() -> None:
    r = _eval(
        topos_evaluate_file(
            EvaluateFileInput(filepath="topos/__init__.py", preferences=_PREFS)
        )
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

    r = _eval(
        topos_evaluate_file(EvaluateFileInput(filepath="module.py", preferences=_PREFS))
    )

    assert r.coupling_available is False
    assert r.warnings
    assert r.agent_contract is not None
    assert "missing_gitnexus_dir" in r.agent_contract.blocked_by
    # The "COMPOSABLE not scored" note now lives in the flat interpretation.
    assert "mdg.unavailable" in r.interpretation


def test_gitnexus_warnings_surface_schema_mismatch(tmp_path: Path) -> None:
    from topos.graphs.mdg.object import LadybugSchemaMismatchError
    from topos.mcp.evaluation import (
        clear_dep_graph_error,
        gitnexus_warnings,
        load_dep_graph,
    )

    gitnexus_dir = tmp_path / ".gitnexus"
    gitnexus_dir.mkdir()
    (gitnexus_dir / "lbug").write_bytes(b"\x00")

    clear_dep_graph_error()
    with patch(
        "topos.mcp.evaluation.dep_graph_for",
        side_effect=LadybugSchemaMismatchError(
            "LadybugDB storage version mismatch while loading .gitnexus/lbug. "
            "Upgrade Topos to v0.3.4+."
        ),
    ):
        dep_graph = load_dep_graph(gitnexus_dir, "module.py")

    assert dep_graph is None
    warnings = gitnexus_warnings(
        str(gitnexus_dir),
        tmp_path,
        gitnexus_dir,
        dep_graph_loaded=False,
    )
    assert any("storage version mismatch" in w.lower() for w in warnings)


def test_dep_graph_cache_keyed_by_mtime(tmp_path: Path) -> None:
    """Second load with unchanged mtime is served from cache; a bumped lbug
    mtime busts it. This is a performance guarantee — caching must never change
    a verdict, only avoid redundant disk reads/parsing."""
    import os

    from topos.graphs.mdg.object import ModuleDependencyGraph
    from topos.mcp import cache
    from topos.mcp.evaluation import load_dep_graph

    gitnexus_dir = tmp_path / ".gitnexus"
    gitnexus_dir.mkdir()
    lbug = gitnexus_dir / "lbug"
    lbug.write_bytes(b"\x00")

    cache.clear_caches()
    fake_graph = MagicMock(spec=ModuleDependencyGraph)
    with patch.object(
        ModuleDependencyGraph,
        "from_gitnexus_dir",
        return_value=fake_graph,
    ) as loader:
        first = load_dep_graph(gitnexus_dir, "module.py")
        second = load_dep_graph(gitnexus_dir, "module.py")
        # Same snapshot mtime -> single underlying load.
        assert loader.call_count == 1
        assert first is second is fake_graph

        # Bump the lbug mtime: the snapshot changed, so the cache must miss.
        bumped = lbug.stat().st_mtime + 10
        os.utime(lbug, (bumped, bumped))
        load_dep_graph(gitnexus_dir, "module.py")
        assert loader.call_count == 2

    cache.clear_caches()


def test_evaluate_file_reports_security_findings(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    from topos.mcp import security

    monkeypatch.setenv("TOPOS_MCP_FILE_ROOT", str(tmp_path))
    security.reset_file_root_cache()
    path = tmp_path / "danger.py"
    path.write_text("def f(expr):\n    return eval(expr)\n", encoding="utf-8")

    r = _eval(
        topos_evaluate_file(EvaluateFileInput(filepath="danger.py", preferences=_PREFS))
    )

    assert r.security_findings
    assert r.security_findings[0].callee == "eval"
    assert r.security_findings[0].line == 2


def test_evaluate_file_applies_topos_allowlist(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    from topos.mcp import security

    monkeypatch.setenv("TOPOS_MCP_FILE_ROOT", str(tmp_path))
    security.reset_file_root_cache()
    path = tmp_path / "danger.py"
    path.write_text("def f(expr):\n    return eval(expr)\n", encoding="utf-8")
    (tmp_path / ".topos.toml").write_text(
        '[[secure.allow]]\npattern = "eval"\nreason = "trusted REPL"\n',
        encoding="utf-8",
    )

    r = _eval(
        topos_evaluate_file(EvaluateFileInput(filepath="danger.py", preferences=_PREFS))
    )

    assert r.secure_raw is False
    assert r.secure_adjusted is True
    assert r.security_findings == []
    assert r.acknowledged_risks
    assert r.acknowledged_risks[0].callee == "eval"
    assert r.acknowledged_risks[0].reason == "trusted REPL"
    assert not [s for s in r.suggestions if s.pillar == "secure"]


def test_evaluate_file_supports_one_off_allow(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    from topos.mcp import security

    monkeypatch.setenv("TOPOS_MCP_FILE_ROOT", str(tmp_path))
    security.reset_file_root_cache()
    path = tmp_path / "danger.py"
    path.write_text("def f(expr):\n    return eval(expr)\n", encoding="utf-8")

    r = _eval(
        topos_evaluate_file(
            EvaluateFileInput(filepath="danger.py", preferences=_PREFS, allow=["eval"])
        )
    )

    assert r.secure_raw is False
    assert r.secure_adjusted is True
    assert r.acknowledged_risks[0].reason == "CLI --allow (ephemeral)"


def test_evaluate_file_rejects_path_outside_root(tmp_path: Path) -> None:
    outside = tmp_path / "stranger.py"
    outside.write_text("x = 1")
    r = _eval(
        topos_evaluate_file(
            EvaluateFileInput(filepath=str(outside), preferences=_PREFS)
        )
    )
    assert r.error is not None
    assert "Access denied" in r.error


def test_evaluate_file_missing_file_errors() -> None:
    r = _eval(
        topos_evaluate_file(
            EvaluateFileInput(filepath="topos/does_not_exist.py", preferences=_PREFS)
        )
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
        r = _eval(
            topos_evaluate_file(
                EvaluateFileInput(
                    filepath="topos/__init__.py",
                    gitnexus_dir="/fake/.gitnexus",
                    preferences=_PREFS,
                )
            )
        )
    mock_load.assert_called_once()
    assert r.coupling_available is True
    assert "composable" in r.scores, (
        "composable dimension must be present when a ModuleDependencyGraph is attached"
    )


# --- topos_evaluate_project ---


def test_evaluate_project_rolls_up_files() -> None:
    r = _project(
        asyncio.run(
            topos_evaluate_project(
                EvaluateProjectInput(path="topos/graphs", limit=10, preferences=_PREFS),
                _StubCtx(),
            )
        )
    )
    assert r.file_count >= 1
    assert r.aggregate_floor_verdict in list(LatticeElement)
    assert r.count <= r.total
    assert r.files, "expected at least one per-file entry"
    assert r.aggregate_explanation
    assert r.guidance
    assert r.agent_contract is not None
    assert r.agent_contract.verification_gates


def test_evaluate_project_paginates() -> None:
    full = _project(
        asyncio.run(
            topos_evaluate_project(
                EvaluateProjectInput(
                    path="topos", limit=5, offset=0, preferences=_PREFS
                ),
                _StubCtx(),
            )
        )
    )
    page2 = _project(
        asyncio.run(
            topos_evaluate_project(
                EvaluateProjectInput(
                    path="topos", limit=5, offset=5, preferences=_PREFS
                ),
                _StubCtx(),
            )
        )
    )
    assert full.total == page2.total
    # Different entries on different pages.
    full_paths = {e.filepath for e in full.files}
    page2_paths = {e.filepath for e in page2.files}
    assert full_paths.isdisjoint(page2_paths) or len(full_paths) < 5


def test_evaluate_project_applies_topos_allowlist(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    from topos.mcp import security

    monkeypatch.setenv("TOPOS_MCP_FILE_ROOT", str(tmp_path))
    security.reset_file_root_cache()
    (tmp_path / "danger.py").write_text(
        "def f(expr):\n    return eval(expr)\n", encoding="utf-8"
    )
    (tmp_path / ".topos.toml").write_text(
        '[[secure.allow]]\npattern = "eval"\nreason = "trusted REPL"\n',
        encoding="utf-8",
    )

    r = _project(
        asyncio.run(
            topos_evaluate_project(
                EvaluateProjectInput(path=".", limit=5, preferences=_PREFS),
                _StubCtx(),
            )
        )
    )

    assert r.files[0].secure_raw is False
    assert r.files[0].secure_adjusted is True
    assert r.files[0].security_findings == []
    assert r.files[0].acknowledged_risks[0].reason == "trusted REPL"
    assert r.aggregate_floor_verdict == r.files[0].lattice_element
    assert r.agent_contract is not None
    assert r.agent_contract.next_actions[0].startswith("start with worst file")


def test_evaluate_project_rejects_outside_root(tmp_path: Path) -> None:
    r = _project(
        asyncio.run(
            topos_evaluate_project(
                EvaluateProjectInput(path=str(tmp_path), limit=5, preferences=_PREFS),
                _StubCtx(),
            )
        )
    )
    # Either refused (path outside root) or empty (no .py files).
    assert r.error is not None or r.file_count == 0
