from __future__ import annotations

import pytest
from topos.core.morphism import ProgramMorphism
from topos.evaluation.policies.coverage import score_topological_coverage
from topos.functors.profunctors.cpg.topological_coverage import (
    calculate_topological_coverage,
    get_embedding_model,
    get_test_scoped_subgraph,
)
from topos.graphs.cpg.object import CodePropertyGraph


def _cpg(source: str, language: str = "python") -> CodePropertyGraph:
    morphism = ProgramMorphism(source=source, language=language)
    cpg = morphism.build_cpg()
    assert cpg is not None
    return cpg


def test_lazy_embedding_model_loading():
    model = get_embedding_model()
    assert model is not None
    # Subsequent calls should return the same singleton instance
    assert get_embedding_model() is model


def test_get_test_scoped_subgraph_call_graph_reachability():
    put_src = (
        "def entry_point():\n"
        "    nested_helper()\n"
        "\n"
        "def nested_helper():\n"
        "    print('work')\n"
        "\n"
        "def isolated_untested():\n"
        "    pass\n"
    )
    test_src = "def test_entry():\n    entry_point()\n"

    put_cpg = _cpg(put_src)
    test_cpg = _cpg(test_src)

    scoped_nodes, tested, untested = get_test_scoped_subgraph(put_cpg, test_cpg)

    assert "entry_point" in tested
    assert "nested_helper" in tested  # Reachable via call inside entry_point
    assert "isolated_untested" in untested
    assert len(scoped_nodes) > 0


def test_topological_coverage_identical_source():
    # Identical structure for PUT and Test should yield very high topological overlap
    src = (
        "def do_calculation(x, y):\n    if x > 0:\n        return x + y\n    return y\n"
    )

    put_cpg = _cpg(src)
    test_cpg = _cpg(src)

    report = calculate_topological_coverage(put_cpg, test_cpg)
    assert report.topological_coverage_score == pytest.approx(1.0)
    assert report.topological_distance == pytest.approx(0.0)
    expected_scoped = sum(1 for n in put_cpg.nodes.values() if n.kind != "File")
    assert report.scoped_node_count == expected_scoped


def test_topological_coverage_partially_covered():
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

    put_cpg = _cpg(put_src)
    test_cpg = _cpg(test_src)

    report = calculate_topological_coverage(put_cpg, test_cpg)

    # Coverage score should be high but since some logic is untouched,
    # the test graph is much smaller, so there is some distance.
    assert 0.0 < report.topological_coverage_score <= 1.0
    assert "calculate_tax" in report.tested_functions
    assert "unused_complex_logic" in report.untested_functions


def test_topological_coverage_empty_graphs():
    put_cpg = CodePropertyGraph()
    test_cpg = CodePropertyGraph()

    report = calculate_topological_coverage(put_cpg, test_cpg)
    assert report.topological_coverage_score == pytest.approx(1.0)
    assert report.topological_distance == pytest.approx(0.0)


def test_score_topological_coverage_decision():
    put_src = "def f(): return 1\n"
    test_src = "def test_f(): f()\n"

    put_cpg = _cpg(put_src)
    test_cpg = _cpg(test_src)

    report = calculate_topological_coverage(put_cpg, test_cpg)
    decision = score_topological_coverage(report, threshold=0.5)

    assert decision.score == report.topological_coverage_score
    assert decision.threshold == 0.5
    assert decision.achieved is True
    assert "ECT L2 distance" in decision.interpretation["topological_distance"]
    assert "functions tested" in decision.interpretation["tested_functions_count"]
