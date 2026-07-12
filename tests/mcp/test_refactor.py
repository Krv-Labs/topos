"""Tests for the unified refactoring suite MCP tools (issues #83, #84, #86)."""

from __future__ import annotations

from topos.mcp.schemas import (
    RefactorCyclesInput,
    RefactorCyclesResult,
    RefactorDependenciesInput,
    RefactorDependenciesResult,
    RefactorProcessInput,
    RefactorProcessResult,
)
from topos.mcp.tools.refactor import (
    topos_refactor_cycles,
    topos_refactor_dependencies,
    topos_refactor_process,
)


def _use_root(tmp_path, monkeypatch) -> None:
    from topos.mcp import security

    monkeypatch.setenv("TOPOS_MCP_FILE_ROOT", str(tmp_path))
    security.reset_file_root_cache()


def _cycles(tool_result) -> RefactorCyclesResult:
    return RefactorCyclesResult.model_validate(tool_result.structured_content)


def _dependencies(tool_result) -> RefactorDependenciesResult:
    return RefactorDependenciesResult.model_validate(tool_result.structured_content)


def _process(tool_result) -> RefactorProcessResult:
    return RefactorProcessResult.model_validate(tool_result.structured_content)


def test_refactor_cycles_finds_loop_and_maps_source_lines(tmp_path, monkeypatch):
    _use_root(tmp_path, monkeypatch)
    source = (
        "def f(items):\n"
        "    total = 0\n"
        "    for x in items:\n"
        "        total += x\n"
        "    return total\n"
    )
    (tmp_path / "loopy.py").write_text(source)

    result = _cycles(topos_refactor_cycles(RefactorCyclesInput(filepath="loopy.py")))

    assert result.error is None
    assert result.betti_1 == 1
    assert len(result.hotspots) == 1
    hotspot = result.hotspots[0]
    assert hotspot.kind == "cycle"
    assert hotspot.filepath == "loopy.py"
    assert hotspot.line_start is not None and hotspot.line_end is not None


def test_refactor_cycles_no_loop_yields_no_hotspots(tmp_path, monkeypatch):
    _use_root(tmp_path, monkeypatch)
    (tmp_path / "flat.py").write_text("def f():\n    return 1\n")

    result = _cycles(topos_refactor_cycles(RefactorCyclesInput(filepath="flat.py")))
    assert result.betti_1 == 0
    assert result.hotspots == []


def test_refactor_cycles_rejects_path_outside_root(tmp_path, monkeypatch):
    _use_root(tmp_path, monkeypatch)
    result = _cycles(topos_refactor_cycles(RefactorCyclesInput(filepath="/etc/passwd")))
    assert result.error is not None


def test_refactor_dependencies_degrades_gracefully_without_gitnexus(
    tmp_path, monkeypatch
):
    _use_root(tmp_path, monkeypatch)
    (tmp_path / "a.py").write_text("x = 1\n")

    result = _dependencies(
        topos_refactor_dependencies(RefactorDependenciesInput(filepath="a.py"))
    )
    assert result.gitnexus_available is False
    assert result.hotspots == []
    assert result.error is None


def test_refactor_process_degrades_gracefully_without_gitnexus(tmp_path, monkeypatch):
    _use_root(tmp_path, monkeypatch)
    (tmp_path / "a.py").write_text("x = 1\n")

    result = _process(topos_refactor_process(RefactorProcessInput(filepath="a.py")))
    assert result.gitnexus_available is False
    assert result.hotspots == []
    assert result.error is None
