"""Tests for topos_inspect_code."""

from __future__ import annotations

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


def test_inspect_top_n_functions_caps_output() -> None:
    code = "\n".join(f"def f{i}():\n    return {i}" for i in range(50))
    r = topos_inspect_code(InspectCodeInput(code=code, top_n_functions=5))
    assert len(r.functions) <= 5
    assert r.total_functions == 50


def test_inspect_entropy_details_populated() -> None:
    r = topos_inspect_code(InspectCodeInput(code="def foo(): return 1\n" * 10))
    assert r.entropy_compression_ratio is not None
