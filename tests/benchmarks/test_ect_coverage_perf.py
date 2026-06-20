"""Performance smoke benchmarks for ECT topological coverage.

Run with metrics output:
  TOPOS_BENCHMARK=1 pytest tests/benchmarks/test_ect_coverage_perf.py -s
"""

from __future__ import annotations

import os
import time

import pytest
from topos.core.morphism import ProgramMorphism
from topos.functors.profunctors.cpg import topological_coverage as tc
from topos.functors.profunctors.cpg.topological_coverage import (
    calculate_topological_coverage,
    ect_coverage_available,
)
from topos.graphs.cpg.object import CodePropertyGraph

pytestmark = pytest.mark.skipif(
    not ect_coverage_available(),
    reason="ect-coverage optional extra not installed",
)


def _cpg(source: str) -> CodePropertyGraph:
    morphism = ProgramMorphism(source=source, language="python")
    cpg = morphism.build_cpg()
    assert cpg is not None
    return cpg


def _bench(
    label: str,
    put_src: str,
    test_src: str,
    *,
    warm_budget_s: float,
) -> dict[str, float | int]:
    t0 = time.perf_counter()
    put_cpg = _cpg(put_src)
    test_cpg = _cpg(test_src)
    t_cpg = time.perf_counter() - t0

    tc._EMBEDDING_MODEL = None
    t0 = time.perf_counter()
    cold_report = calculate_topological_coverage(put_cpg, test_cpg)
    t_cold = time.perf_counter() - t0

    t0 = time.perf_counter()
    warm_report = calculate_topological_coverage(put_cpg, test_cpg)
    t_warm = time.perf_counter() - t0

    metrics = {
        "label": label,
        "scoped_nodes": cold_report.scoped_node_count,
        "put_nodes": cold_report.put_node_count,
        "test_nodes": cold_report.test_node_count,
        "t_cpg_s": t_cpg,
        "t_cold_s": t_cold,
        "t_warm_s": t_warm,
    }
    if os.environ.get("TOPOS_BENCHMARK"):
        print(
            f"{label}: scoped={metrics['scoped_nodes']} "
            f"cpg={t_cpg:.3f}s cold={t_cold:.3f}s warm={t_warm:.3f}s"
        )

    assert (
        warm_report.topological_coverage_score
        == cold_report.topological_coverage_score
    )
    assert t_warm <= warm_budget_s, (
        f"{label} warm path exceeded {warm_budget_s}s budget ({t_warm:.3f}s)"
    )
    return metrics


def test_ect_coverage_perf_tiny_pair():
    put_src = "def add(a, b):\n    return a + b\n"
    test_src = "def test_add():\n    add(1, 2)\n"
    _bench("tiny", put_src, test_src, warm_budget_s=5.0)


def test_ect_coverage_perf_medium_pair():
    put_src = (
        "def calculate_tax(amount):\n"
        "    if amount > 1000:\n"
        "        return amount * 0.15\n"
        "    return amount * 0.1\n"
        "\n"
        "def unused_complex_logic():\n"
        "    for i in range(10):\n"
        "        if i % 2 == 0:\n"
        "            print(i)\n"
    )
    test_src = "def test_tax():\n    calculate_tax(500)\n"
    _bench("medium", put_src, test_src, warm_budget_s=8.0)


def test_ect_coverage_perf_branchy_pair():
    put_src = (
        "def route(x):\n"
        "    if x < 0:\n"
        "        return 'neg'\n"
        "    if x == 0:\n"
        "        return 'zero'\n"
        "    for i in range(x):\n"
        "        if i % 3 == 0:\n"
        "            continue\n"
        "        if i % 5 == 0:\n"
        "            break\n"
        "    return 'pos'\n"
    ) * 3
    test_src = "def test_route():\n    assert route(2) == 'pos'\n"
    _bench("branchy", put_src, test_src, warm_budget_s=15.0)
