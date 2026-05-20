"""Tests for topos_inspect_code."""

from __future__ import annotations

from pathlib import Path

import pytest
from topos.mcp.schemas import InspectCodeInput
from topos.mcp.tools.inspect import topos_inspect_code


def test_inspect_returns_function_table() -> None:
    code = """
def a(): return 1
def b(x):
    if x:
        return 1
    return 2
def c(x, y):
    if x:
        if y:
            return 1
        return 2
    return 3
"""
    r = topos_inspect_code(InspectCodeInput(code=code))
    assert r.total_functions == 3
    assert set(r.functions.keys()) <= {"a", "b", "c"}
    assert r.function_entries
    assert {entry.name for entry in r.function_entries} <= {"a", "b", "c"}
    assert all(entry.line > 0 for entry in r.function_entries)


def test_inspect_top_n_functions_caps_output() -> None:
    code = "\n".join(f"def f{i}():\n    return {i}" for i in range(50))
    r = topos_inspect_code(InspectCodeInput(code=code, top_n_functions=5))
    assert len(r.functions) <= 5
    assert r.total_functions == 50


def test_inspect_entropy_details_populated() -> None:
    r = topos_inspect_code(InspectCodeInput(code="def foo(): return 1\n" * 10))
    assert r.entropy_compression_ratio is not None


def test_inspect_accepts_filepath(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    from topos.mcp import security

    monkeypatch.setenv("TOPOS_MCP_FILE_ROOT", str(tmp_path))
    security.reset_file_root_cache()
    path = tmp_path / "module.py"
    path.write_text(
        "def f(x):\n    if x:\n        return 1\n    return 0\n",
        encoding="utf-8",
    )

    r = topos_inspect_code(InspectCodeInput(filepath="module.py"))

    assert r.error is None
    assert r.function_entries[0].name == "f"
    assert r.function_entries[0].line == 1


def test_inspect_requires_exactly_one_source() -> None:
    with pytest.raises(ValueError, match="code.*filepath"):
        InspectCodeInput()
