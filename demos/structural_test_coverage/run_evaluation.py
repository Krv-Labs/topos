#!/usr/bin/env python3
# ruff: noqa: I001
"""Run structural test coverage on synthetic pairs and binarytrees-style PUT."""

from __future__ import annotations

import json
import sys
from dataclasses import asdict
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]

if str(ROOT / "src") not in sys.path:
    sys.path.insert(0, str(ROOT / "src"))

from topos.functors.profunctors.uast import (  # noqa: E402
    declaration_coverage,
    structural_test_coverage,
)
from topos.graphs.ast.dispatch import parse_source  # noqa: E402


def _parse_py(source: str) -> object:
    root = parse_source(source=source, language="python", file="<eval>").uast_root
    if root is None:
        msg = "Expected UAST root for Python snippet"
        raise RuntimeError(msg)
    return root


def _print_block(title: str, report: object) -> None:
    print(title)
    print("-" * len(title))
    d = asdict(report)
    for key in (
        "kind_recall",
        "control_flow_recall",
        "composite_v0",
        "path_recall_kgram",
        "put_kind_nodes",
        "test_kind_nodes",
        "put_kgram_mass",
        "test_kgram_mass",
    ):
        val = d[key]
        if isinstance(val, float):
            print(f"  {key}: {val:.4f}")
        else:
            print(f"  {key}: {val}")
    print()


def _print_declaration_block(title: str, report: object) -> None:
    print(title)
    print("-" * len(title))
    d = asdict(report)
    for key in (
        "mean_declaration_coverage",
        "declaration_coverage_rate",
        "stmt_recall",
        "expr_recall",
        "mean_test_precision",
        "f2_score",
        "declaration_path_recall_kgram",
        "put_declaration_count",
        "test_declaration_count",
    ):
        val = d[key]
        if isinstance(val, float):
            print(f"  {key}: {val:.4f}")
        else:
            print(f"  {key}: {val}")
    if d["uncovered_declarations"]:
        print("  uncovered_declarations:")
        for location, score in d["uncovered_declarations"]:
            print(f"    {location}: {score:.4f}")
    print()


def main() -> None:
    put_algo = (
        "def total(xs):\n"
        "    s = 0\n"
        "    for x in xs:\n"
        "        if x > 0:\n"
        "            s += x\n"
        "    return s\n"
    )
    put = _parse_py(put_algo)

    minimal_test = "def test_trivial():\n    assert 1 == 1\n"
    loop_test = (
        "def test_like_put():\n"
        "    acc = 0\n"
        "    for y in [1, 2]:\n"
        "        if y > 0:\n"
        "            acc += y\n"
        "    assert acc == 3\n"
    )

    r_min = structural_test_coverage([put], [_parse_py(minimal_test)], k=3)
    r_loop = structural_test_coverage([put], [_parse_py(loop_test)], k=3)
    d_min = declaration_coverage([put], [_parse_py(minimal_test)], k=3)
    d_loop = declaration_coverage([put], [_parse_py(loop_test)], k=3)

    _print_block("Synthetic: PUT list summer vs minimal test", r_min)
    _print_block("Synthetic: PUT list summer vs loop-heavy test", r_loop)
    _print_declaration_block(
        "Synthetic declaration coverage: PUT vs minimal test", d_min
    )
    _print_declaration_block(
        "Synthetic declaration coverage: PUT vs loop-heavy test", d_loop
    )

    bt_path = ROOT / "demos" / "binarytrees" / "src" / "binarytrees.py"
    if bt_path.is_file():
        put_bt = parse_source(
            source=bt_path.read_text(encoding="utf-8"),
            language="python",
            file=str(bt_path),
        ).uast_root
        if put_bt is None:
            print("binarytrees: skip (no UAST root)", file=sys.stderr)
        else:
            thin = "def test_smoke():\n    assert 2 + 2 == 4\n"
            rich = (
                "def test_shape():\n"
                "    d = 4\n"
                "    for i in range(d):\n"
                "        if i >= 0:\n"
                "            assert i < d\n"
                "    return\n"
            )
            r_thin = structural_test_coverage([put_bt], [_parse_py(thin)], k=3)
            r_rich = structural_test_coverage([put_bt], [_parse_py(rich)], k=3)
            d_thin = declaration_coverage([put_bt], [_parse_py(thin)], k=3)
            d_rich = declaration_coverage([put_bt], [_parse_py(rich)], k=3)
            _print_block("binarytrees.py PUT vs thin smoke test", r_thin)
            _print_block("binarytrees.py PUT vs richer control-flow test", r_rich)
            _print_declaration_block(
                "binarytrees.py declaration coverage vs thin smoke test", d_thin
            )
            _print_declaration_block(
                "binarytrees.py declaration coverage vs richer control-flow test",
                d_rich,
            )

            delta = {
                "kind_recall_delta": r_rich.kind_recall - r_thin.kind_recall,
                "cf_recall_delta": (
                    r_rich.control_flow_recall - r_thin.control_flow_recall
                ),
                "path_recall_delta": (
                    r_rich.path_recall_kgram - r_thin.path_recall_kgram
                ),
                "mean_declaration_coverage_delta": (
                    d_rich.mean_declaration_coverage
                    - d_thin.mean_declaration_coverage
                ),
                "declaration_f2_delta": d_rich.f2_score - d_thin.f2_score,
            }
            print("binarytrees deltas (rich - thin)")
            print("-" * 32)
            print(json.dumps(delta, indent=2))
    else:
        print("binarytrees.py not found; skipped.", file=sys.stderr)


if __name__ == "__main__":
    main()
