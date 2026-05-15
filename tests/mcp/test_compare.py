"""Tests for topos_compare_code, topos_compare_files."""

from __future__ import annotations

from pathlib import Path

from topos.mcp.schemas import CompareCodeInput, CompareFilesInput
from topos.mcp.tools.compare import topos_compare_code, topos_compare_files


def test_compare_code_identical_distance_zero() -> None:
    code = "def f(): return 1"
    r = topos_compare_code(CompareCodeInput(source_code=code, target_code=code))
    assert r.source_valid and r.target_valid
    assert r.normalized_distance == 0.0
    assert r.similarity == 1.0


def test_compare_code_reports_distance() -> None:
    r = topos_compare_code(
        CompareCodeInput(
            source_code="def f(x):\n    return x\n",
            target_code="def f(x, y):\n    if x > 0:\n        return y\n    return x\n",
        )
    )
    assert r.normalized_distance > 0.0
    assert 0.0 <= r.similarity <= 1.0


def test_compare_files_reads_from_disk() -> None:
    r = topos_compare_files(
        CompareFilesInput(
            source="src/topos/__init__.py",
            target="src/topos/__init__.py",
        )
    )
    assert r.error is None
    assert r.normalized_distance == 0.0


def test_compare_files_rejects_outside_root(tmp_path: Path) -> None:
    outside = tmp_path / "x.py"
    outside.write_text("x = 1")
    r = topos_compare_files(
        CompareFilesInput(source=str(outside), target="src/topos/__init__.py")
    )
    assert r.error is not None
