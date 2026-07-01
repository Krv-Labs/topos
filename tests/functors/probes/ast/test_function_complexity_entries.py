"""Tests for ``calculate_function_complexity_entries`` (issue #67).

Covers nested functions / closures, methods, and module-level code so the
``ast.max_function_complexity`` gate can always be mapped back to a span.
"""

from __future__ import annotations

from topos.core.object import ProgramObject
from topos.functors.probes.ast.complexity import (
    calculate_function_complexity_entries,
    calculate_max_function_complexity,
)
from topos.utils.tree_sitter import parse_python


def _entries(code: str):
    obj = ProgramObject(root=parse_python(code), source=code, language="python")
    return {e.qualified_name: e for e in calculate_function_complexity_entries(obj)}


def test_top_level_function_kind_and_span() -> None:
    code = "def foo(x):\n    if x:\n        return 1\n    return 0\n"
    entries = _entries(code)
    foo = entries["foo"]
    assert foo.kind == "function"
    assert foo.name == "foo"
    assert foo.start_line == 1
    assert foo.end_line == 4
    assert foo.complexity == 2  # one `if`
    assert foo.includes_nested is True


def test_async_function_kind() -> None:
    entries = _entries("async def bar():\n    return 1\n")
    assert entries["bar"].kind == "async_function"


def test_method_inside_class() -> None:
    code = "class C:\n    def m(self, x):\n        if x:\n            return 1\n"
    entries = _entries(code)
    assert entries["C.m"].kind == "method"


def test_nested_closure_is_dotted_and_outer_includes_nested() -> None:
    code = (
        "class Calc:\n"
        "    def compute(self, x):\n"
        "        if x > 0:\n"
        "            def helper(y):\n"
        "                if y > 1:\n"
        "                    return y\n"
        "                return 0\n"
        "            return helper(x)\n"
        "        return 0\n"
    )
    entries = _entries(code)
    assert "Calc.compute" in entries
    assert "Calc.compute.helper" in entries

    compute = entries["Calc.compute"]
    helper = entries["Calc.compute.helper"]
    assert compute.kind == "method"
    assert helper.kind == "closure"
    # The outer count walks the whole subtree, so it includes the nested
    # callable's decision nodes.
    assert compute.includes_nested is True
    assert compute.complexity >= helper.complexity


def test_module_level_only_has_no_function_entries() -> None:
    code = "x = 1\nif x:\n    y = 2\nelse:\n    y = 3\n"
    obj = ProgramObject(root=parse_python(code), source=code, language="python")
    assert calculate_function_complexity_entries(obj) == []


def test_max_entry_matches_gate_metric() -> None:
    code = "def big(x):\n" + "".join(
        f"    if x == {i}:\n        return {i}\n" for i in range(12)
    )
    obj = ProgramObject(root=parse_python(code), source=code, language="python")
    entries = calculate_function_complexity_entries(obj)
    assert max(e.complexity for e in entries) == calculate_max_function_complexity(obj)
