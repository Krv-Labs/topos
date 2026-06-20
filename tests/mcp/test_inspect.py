"""Tests for topos_inspect_code."""

from __future__ import annotations

from pathlib import Path

import pytest
from topos.mcp.schemas import InspectCodeInput, InspectionResult
from topos.mcp.tools.inspect import topos_inspect_code


def _inspect(tool_result) -> InspectionResult:
    """Rebuild the InspectionResult model from a tool's ToolResult channel."""
    return InspectionResult.model_validate(tool_result.structured_content)


def _content_text(tool_result) -> str:
    """The markdown text the LLM sees (first content block)."""
    return tool_result.content[0].text


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
    r = _inspect(topos_inspect_code(InspectCodeInput(code=code)))
    assert r.total_functions == 3
    assert set(r.functions.keys()) <= {"a", "b", "c"}
    assert r.function_entries
    assert {entry.name for entry in r.function_entries} <= {"a", "b", "c"}
    assert all(entry.line > 0 for entry in r.function_entries)


def test_inspect_top_n_functions_caps_output() -> None:
    code = "\n".join(f"def f{i}():\n    return {i}" for i in range(50))
    r = _inspect(topos_inspect_code(InspectCodeInput(code=code, top_n_functions=5)))
    assert len(r.functions) <= 5
    assert r.total_functions == 50


def test_inspect_entropy_details_populated() -> None:
    r = _inspect(
        topos_inspect_code(InspectCodeInput(code="def foo(): return 1\n" * 10))
    )
    assert r.entropy_compression_ratio is not None


def test_inspect_returns_markdown_content_and_structured() -> None:
    tr = topos_inspect_code(InspectCodeInput(code="def foo(): return 1\n"))
    text = _content_text(tr)
    # Content block is compact markdown, NOT serialized JSON.
    assert not text.lstrip().startswith("{")
    # Structured channel carries the key field.
    assert "total_functions" in tr.structured_content


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

    r = _inspect(topos_inspect_code(InspectCodeInput(filepath="module.py")))

    assert r.error is None
    assert r.function_entries[0].name == "f"
    assert r.function_entries[0].line == 1


def test_inspect_non_ascii_before_def_keeps_clean_names() -> None:
    # Regression: UAST spans are UTF-8 byte offsets. A non-ASCII char (→, —)
    # BEFORE the def shifts byte vs. code-point offsets, which used to slice
    # garbled fragments (e.g. "s(\n    override:") into the function name.
    code = (
        '"""Docstring with → and — and an emoji 🎯 ahead of the def."""\n'
        "def resolve_gitnexus_dir(override):\n    return override\n"
    )
    tr = topos_inspect_code(InspectCodeInput(code=code))
    r = _inspect(tr)
    names = {entry.name for entry in r.function_entries}
    assert "resolve_gitnexus_dir" in names
    # No shifted fragments: every name is a bare identifier.
    assert all(name.isidentifier() for name in names), names
    # Markdown table rows stay well-formed: 3 pipes (4 cells) and no stray
    # newline/pipe inside a name cell.
    text = _content_text(tr)
    body_rows = [
        ln
        for ln in text.splitlines()
        if ln.startswith("| `") and "resolve_gitnexus_dir" in ln
    ]
    assert body_rows
    for row in body_rows:
        # A well-formed 3-column row has exactly 4 unescaped pipes. A stray
        # newline/pipe in the name cell (the old bug) would change this count.
        assert row.count("|") == 4


def test_inspect_requires_exactly_one_source() -> None:
    with pytest.raises(ValueError, match="code.*filepath"):
        InspectCodeInput()
