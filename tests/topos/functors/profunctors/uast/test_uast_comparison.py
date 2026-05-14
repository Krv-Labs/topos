from __future__ import annotations

from dataclasses import replace
from pathlib import Path

from topos.functors.probes.uast import (
    control_flow_profile,
    structural_summary,
    uast_kind_histogram,
)
from topos.functors.profunctors.uast import (
    compare_uast,
    uast_edit_distance,
    uast_kind_distance,
)
from topos.graphs.ast.dispatch import parse_source
from topos.graphs.uast.models import NativeRef, SourceSpan, UASTNode


def _parse(source: str, language: str):
    return parse_source(source=source, language=language).uast_root


def test_identical_uast_has_zero_distance():
    root = _parse("def add(a, b):\n    return a + b\n", language="python")

    assert uast_kind_distance(root, root) == 0.0
    edit = uast_edit_distance(root, root)
    assert edit.raw_distance == 0
    assert edit.normalized_distance == 0.0

    comparison = compare_uast(root, root)
    assert comparison.detects_difference is False
    assert all(value == 0 for value in comparison.control_flow_delta.values())
    assert all(value == 0 for value in comparison.summary_delta.values())


def test_python_vs_rust_binarytrees_differ():
    py_source = Path("demos/binarytrees/src/binarytrees.py").read_text()
    rs_source = Path("demos/binarytrees/src/binarytrees.rs").read_text()

    py_root = _parse(py_source, language="python")
    rs_root = _parse(rs_source, language="rust")

    comparison = compare_uast(py_root, rs_root)

    assert comparison.kind_distance > 0.0
    assert comparison.edit_distance.raw_distance > 0
    assert any(value != 0 for value in comparison.control_flow_delta.values())
    assert comparison.detects_difference is True


def test_kind_histogram_excludes_unknown_when_requested():
    rs_root = _parse(
        'fn main() { let x = 1; if x > 0 { println!("ok"); } }',
        language="rust",
    )

    with_unknown = uast_kind_histogram(rs_root, include_unknown=True)
    without_unknown = uast_kind_histogram(rs_root, include_unknown=False)

    assert with_unknown.get("Unknown", 0) > 0
    assert "Unknown" not in without_unknown
    assert sum(without_unknown.values()) < sum(with_unknown.values())


def test_control_flow_profile_counts_loops_and_returns():
    root = _parse(
        "def f(xs):\n"
        "    total = 0\n"
        "    for x in xs:\n"
        "        if x > 0:\n"
        "            total += x\n"
        "    return total\n",
        language="python",
    )

    profile = control_flow_profile(root)
    assert profile["ForStmt"] == 1
    assert profile["IfStmt"] == 1
    assert profile["ReturnStmt"] == 1


def test_structural_summary_counts_declarations():
    root = _parse(
        "def a(): pass\ndef b(): pass\nclass C:\n    def m(self): pass\n",
        language="python",
    )

    summary = structural_summary(root)
    assert summary.node_count > 0
    assert summary.depth > 0
    assert summary.declaration_count >= 3  # two functions + one class


def _collect_ids(node) -> list[str]:
    ids = [node.id]
    for child in node.children:
        ids.extend(_collect_ids(child))
    return ids


def test_uast_node_id_is_deterministic():
    source = "def f(x):\n    return x + 1\n"
    root_a = _parse(source, language="python")
    root_b = _parse(source, language="python")

    ids_a = _collect_ids(root_a)
    ids_b = _collect_ids(root_b)

    assert ids_a == ids_b
    assert all(len(node_id) == 16 for node_id in ids_a)


def test_uast_node_id_is_unique_within_tree():
    source = Path("demos/binarytrees/src/binarytrees.py").read_text()
    root = _parse(source, language="python")

    ids = _collect_ids(root)
    assert len(ids) == len(set(ids))


def test_uast_node_id_differs_across_languages():
    py_root = _parse("x = 1\n", language="python")
    js_root = _parse("x = 1\n", language="javascript")

    assert py_root.id != js_root.id


def test_compare_uast_handles_empty_uast_node():
    span = SourceSpan(
        file=None,
        start_byte=0,
        end_byte=0,
        start_line=1,
        start_column=0,
        end_line=1,
        end_column=0,
    )
    native = NativeRef(parser="test", parser_version="0", node_kind="module")
    empty = UASTNode(kind="File", lang="python", span=span, native=native)
    other = replace(empty, kind="Unknown")

    comparison = compare_uast(empty, other)
    assert comparison.kind_distance == 1.0
    assert comparison.edit_distance.raw_distance == 1
