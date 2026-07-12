"""Tests for the unified refactoring suite MCP tool (issues #83, #84, #86)."""

from __future__ import annotations

from topos.mcp.schemas import RefactorInput, RefactorResult
from topos.mcp.tools.refactor import topos_refactor


def _use_root(tmp_path, monkeypatch) -> None:
    from topos.mcp import security

    monkeypatch.setenv("TOPOS_MCP_FILE_ROOT", str(tmp_path))
    security.reset_file_root_cache()


def _result(tool_result) -> RefactorResult:
    return RefactorResult.model_validate(tool_result.structured_content)


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

    result = _result(
        topos_refactor(RefactorInput(target="cycles", filepath="loopy.py"))
    )

    assert result.target == "cycles"
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

    result = _result(topos_refactor(RefactorInput(target="cycles", filepath="flat.py")))
    assert result.betti_1 == 0
    assert result.hotspots == []


def test_refactor_cycles_rejects_path_outside_root(tmp_path, monkeypatch):
    _use_root(tmp_path, monkeypatch)
    result = _result(
        topos_refactor(RefactorInput(target="cycles", filepath="/etc/passwd"))
    )
    assert result.error is not None


def test_refactor_dependencies_degrades_gracefully_without_gitnexus(
    tmp_path, monkeypatch
):
    _use_root(tmp_path, monkeypatch)
    (tmp_path / "a.py").write_text("x = 1\n")

    result = _result(
        topos_refactor(RefactorInput(target="dependencies", filepath="a.py"))
    )
    assert result.target == "dependencies"
    assert result.gitnexus_available is False
    assert result.hotspots == []
    assert result.error is None


def test_refactor_process_degrades_gracefully_without_gitnexus(tmp_path, monkeypatch):
    _use_root(tmp_path, monkeypatch)
    (tmp_path / "a.py").write_text("x = 1\n")

    result = _result(topos_refactor(RefactorInput(target="process", filepath="a.py")))
    assert result.target == "process"
    assert result.gitnexus_available is False
    assert result.hotspots == []
    assert result.error is None


def test_refactor_limit_defaults_and_caps():
    params = RefactorInput(target="cycles", filepath="x.py")
    assert params.limit == 5
    for bad in (0, 51):
        try:
            RefactorInput(target="cycles", filepath="x.py", limit=bad)
        except Exception:
            continue
        raise AssertionError(f"limit={bad} should have been rejected")
