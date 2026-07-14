"""Tests for ``calculate_function_complexity_entries`` (issue #67).

Covers nested functions / closures, methods, and module-level code so the
``ast.max_function_complexity`` gate can always be mapped back to a span.

``_entries`` builds the ``ProgramObject`` via ``ProgramMorphism`` (rather
than constructing it directly), so these tests exercise the real production
code path -- ``uast_root`` populated -- which is what actually drives
``ast.max_function_complexity`` for real callers (see issue #153).
"""

from __future__ import annotations

from topos.core.morphism import ProgramMorphism
from topos.core.object import ProgramObject
from topos.functors.probes.ast.complexity import (
    calculate_function_complexity_entries,
    calculate_max_function_complexity,
)
from topos.utils.tree_sitter import parse_python


def _entries(code: str):
    obj = ProgramMorphism(source=code, language="python").ast
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
    obj = ProgramMorphism(source=code, language="python").ast
    assert calculate_function_complexity_entries(obj) == []


def test_max_entry_matches_gate_metric() -> None:
    code = "def big(x):\n" + "".join(
        f"    if x == {i}:\n        return {i}\n" for i in range(12)
    )
    obj = ProgramMorphism(source=code, language="python").ast
    entries = calculate_function_complexity_entries(obj)
    assert max(e.complexity for e in entries) == calculate_max_function_complexity(obj)


# ---------------------------------------------------------------------------
# Legacy fallback: a ProgramObject constructed directly (uast_root=None).
# ---------------------------------------------------------------------------
#
# Real callers always go through ProgramMorphism / parse_source, which
# populate uast_root for every supported language -- but complexity.py keeps
# a defensive, Python-only native tree-sitter fallback for ProgramObjects
# built without one. This guards that fallback from silently rotting.


def test_fallback_path_used_when_uast_root_is_none() -> None:
    code = "def foo(x):\n    if x:\n        return 1\n    return 0\n"
    obj = ProgramObject(root=parse_python(code), source=code, language="python")
    assert obj.uast_root is None

    entries = {e.qualified_name: e for e in calculate_function_complexity_entries(obj)}
    foo = entries["foo"]
    assert foo.kind == "function"
    assert foo.complexity == 2  # one `if`
    assert calculate_max_function_complexity(obj) == 2


def test_python_program_morphism_keeps_native_decision_coverage() -> None:
    code = (
        "def f(xs):\n"
        "    if xs:\n"
        "        pass\n"
        "    for x in xs:\n"
        "        pass\n"
        "    while xs:\n"
        "        break\n"
        "    try:\n"
        "        risky()\n"
        "    except Exception:\n"
        "        pass\n"
        "    with open('x') as fh:\n"
        "        pass\n"
        "    assert xs\n"
        "    y = 1 if xs else 0\n"
        "    z = [i for i in xs if i]\n"
        "    match y:\n"
        "        case 1:\n"
        "            return 1\n"
        "        case 2:\n"
        "            return 2\n"
        "        case _:\n"
        "            return 3\n"
    )
    ast = ProgramMorphism(source=code, language="python").ast
    assert calculate_max_function_complexity(ast) == 13
