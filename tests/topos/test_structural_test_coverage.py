from dataclasses import FrozenInstanceError

import pytest

from topos.graphs.ast.dispatch import parse_source
from topos.functors.probes.uast.structural_test_coverage import (
    DeclarationCoverageReport,
    StructuralTestCoverageReport,
    declaration_coverage,
    extract_declarations,
    merge_uast_kind_histograms,
    structural_test_coverage,
)


def _parse(source: str, language: str = "python"):
    return parse_source(source=source, language=language).uast_root


def test_identical_put_and_test_full_recall():
    src = (
        "def f(n):\n"
        "    for i in range(n):\n"
        "        if i % 2:\n"
        "            pass\n"
        "    return n\n"
    )
    put = _parse(src)
    rep = structural_test_coverage([put], [put], k=3, include_unknown=False)
    assert rep.kind_recall == pytest.approx(1.0)
    assert rep.control_flow_recall == pytest.approx(1.0)
    assert rep.composite_v0 == pytest.approx(1.0)
    assert rep.path_recall_kgram == pytest.approx(1.0)


def test_empty_tests_yield_zero_kind_recall_when_put_nonempty():
    put = _parse("def g():\n    while True:\n        break\n")
    rep = structural_test_coverage([put], [], k=3, include_unknown=False)
    assert rep.kind_recall == pytest.approx(0.0)
    assert rep.control_flow_recall == pytest.approx(0.0)
    assert rep.composite_v0 == pytest.approx(0.0)
    assert rep.path_recall_kgram == pytest.approx(0.0)


def test_tests_with_loop_improve_recall_vs_minimal_tests():
    put = _parse(
        "def total(xs):\n"
        "    s = 0\n"
        "    for x in xs:\n"
        "        if x > 0:\n"
        "            s += x\n"
        "    return s\n"
    )
    minimal_tests = _parse("def test_const():\n    assert 1 == 1\n")
    richer_tests = _parse(
        "def test_sum():\n"
        "    acc = 0\n"
        "    for y in [1, 2]:\n"
        "        if y > 0:\n"
        "            acc += y\n"
        "    assert acc == 3\n"
    )
    low = structural_test_coverage([put], [minimal_tests], k=3, include_unknown=False)
    high = structural_test_coverage([put], [richer_tests], k=3, include_unknown=False)
    assert high.kind_recall >= low.kind_recall
    assert high.control_flow_recall > low.control_flow_recall
    assert high.path_recall_kgram >= low.path_recall_kgram


def test_path_recall_moves_with_shared_kgrams():
    put = _parse("def a():\n    if True:\n        return 1\n")
    disjoint = _parse("def t():\n    x = 1\n    y = 2\n    return x + y\n")
    aligned = _parse("def t():\n    if True:\n        return 2\n")
    low = structural_test_coverage([put], [disjoint], k=3, include_unknown=False)
    high = structural_test_coverage([put], [aligned], k=3, include_unknown=False)
    assert high.path_recall_kgram > low.path_recall_kgram


def test_invalid_k_raises():
    put = _parse("def f(): return 0\n")
    with pytest.raises(ValueError, match="k must be"):
        structural_test_coverage([put], [put], k=0)


def test_merge_histograms_two_roots():
    a = _parse("def a(): pass\n")
    b = _parse("def b(): return 1\n")
    merged = merge_uast_kind_histograms([a, b], include_unknown=False)
    single = merge_uast_kind_histograms([a], include_unknown=False)
    assert sum(merged.values()) > sum(single.values())


def test_report_is_frozen_dataclass():
    put = _parse("pass\n")
    rep = structural_test_coverage([put], [put], k=2)
    assert isinstance(rep, StructuralTestCoverageReport)
    with pytest.raises(FrozenInstanceError):
        rep.kind_recall = 0.0  # type: ignore[misc]


# ---------------------------------------------------------------------------
# v2: declaration_coverage
# ---------------------------------------------------------------------------


def test_decl_coverage_identical_put_and_test_full_scores():
    src = (
        "def f(n):\n"
        "    for i in range(n):\n"
        "        if i % 2:\n"
        "            pass\n"
        "    return n\n"
    )
    root = _parse(src)
    rep = declaration_coverage([root], [root], k=3, include_unknown=False)
    assert rep.mean_declaration_coverage == pytest.approx(1.0)
    assert rep.declaration_coverage_rate == pytest.approx(1.0)
    assert rep.stmt_recall == pytest.approx(1.0)
    assert rep.declaration_path_recall_kgram == pytest.approx(1.0)
    assert len(rep.uncovered_declarations) == 0


def test_decl_coverage_empty_tests_yield_zero():
    src = (
        "def g():\n    while True:\n        if True:\n            break\n    return 0\n"
    )
    put = _parse(src)
    rep = declaration_coverage([put], [], k=3)
    assert rep.mean_declaration_coverage == pytest.approx(0.0)
    assert rep.declaration_coverage_rate == pytest.approx(0.0)
    assert rep.mean_test_precision == pytest.approx(0.0)
    assert rep.f2_score == pytest.approx(0.0)
    assert len(rep.uncovered_declarations) == rep.put_declaration_count


def test_decl_coverage_unrelated_test_does_not_inflate():
    # PUT: loop-heavy function; test: pure arithmetic — structurally disjoint
    put = _parse(
        "def process(xs):\n"
        "    result = []\n"
        "    for x in xs:\n"
        "        if x > 0:\n"
        "            result.append(x)\n"
        "    return result\n"
    )
    unrelated_test = _parse(
        "def test_math():\n    a = 1 + 2\n    b = a * 3\n    assert b == 9\n"
    )
    rep = declaration_coverage([put], [unrelated_test], k=3)
    # An unrelated test should not fully cover the loop-heavy PUT declaration
    assert rep.mean_declaration_coverage < 1.0


def test_decl_coverage_bloated_test_does_not_fully_cover_focused_put():
    # PUT: complex function; tests: many trivial stubs that share nothing
    put = _parse(
        "def compute(data):\n"
        "    total = 0\n"
        "    for item in data:\n"
        "        if item > 0:\n"
        "            total += item\n"
        "        elif item < 0:\n"
        "            total -= item\n"
        "    return total\n"
    )
    # Bloated test file: many tiny stubs that share virtually no structure with PUT
    bloated = _parse(
        "def test_a(): pass\n"
        "def test_b(): pass\n"
        "def test_c(): pass\n"
        "def test_d(): pass\n"
        "def test_e(): pass\n"
    )
    rep_bloated = declaration_coverage([put], [bloated], k=3)
    tight_test = _parse(
        "def test_compute():\n"
        "    total = 0\n"
        "    for item in [1, -2, 3]:\n"
        "        if item > 0:\n"
        "            total += item\n"
        "        elif item < 0:\n"
        "            total -= item\n"
        "    assert total == 2\n"
    )
    rep_tight = declaration_coverage([put], [tight_test], k=3)
    # Tight, focused test should outperform bloated test suite
    assert rep_tight.mean_declaration_coverage > rep_bloated.mean_declaration_coverage


def test_decl_coverage_precision_tight_vs_bloated():
    put = _parse("def f(x):\n    if x > 0:\n        return x\n    return -x\n")
    tight = _parse("def test_f():\n    if True:\n        return 1\n    return -1\n")
    # Bloated suite: one aligned test + several structurally unrelated functions
    # (non-empty bodies so they don't get vacuous 1.0 precision)
    bloated = _parse(
        "def test_a():\n"
        "    x = 1 + 2 + 3 + 4 + 5\n"
        "    y = x * x * x\n"
        "    return y\n"
        "def test_b():\n"
        "    items = [1, 2, 3]\n"
        "    total = items[0] + items[1] + items[2]\n"
        "    return total\n"
        "def test_f():\n"
        "    if True:\n"
        "        return 1\n"
        "    return -1\n"
    )
    rep_tight = declaration_coverage([put], [tight], k=3)
    rep_bloated = declaration_coverage([put], [bloated], k=3)
    # Tight suite has higher precision (no irrelevant test mass pulling mean down)
    assert rep_tight.mean_test_precision > rep_bloated.mean_test_precision


def test_decl_coverage_f2_emphasizes_recall():
    # Scenario: recall is high, precision is moderate
    # F2 should be closer to recall than to precision
    put = _parse("def f():\n    for i in range(10):\n        pass\n    return 0\n")
    test_src = _parse("def t():\n    for i in range(10):\n        pass\n    return 0\n")
    rep = declaration_coverage([put], [test_src], k=3)
    if rep.mean_test_precision > 0 and rep.mean_declaration_coverage > 0:
        # F2 is at least as close to recall as to precision when recall > precision
        # or equivalently f2 >= precision when beta=2
        assert rep.f2_score >= rep.mean_test_precision * 0.9


def test_decl_coverage_category_stratified_disjoint():
    # PUT with only statement kinds (loops, conditionals) — no expression-only code
    put = _parse(
        "def f():\n"
        "    for i in range(3):\n"
        "        if i > 0:\n"
        "            continue\n"
        "    return None\n"
    )
    test_src = _parse(
        "def t():\n"
        "    for i in range(3):\n"
        "        if i > 0:\n"
        "            continue\n"
        "    return None\n"
    )
    rep = declaration_coverage([put], [test_src], k=3)
    # Stmt recall should be positive (both have loops/conditionals)
    assert rep.stmt_recall > 0.0
    # Stmt and expr use disjoint kind subsets — both are valid floats in [0,1]
    assert 0.0 <= rep.stmt_recall <= 1.0
    assert 0.0 <= rep.expr_recall <= 1.0


def test_extract_declarations_finds_function_and_method():
    src = (
        "class MyClass:\n"
        "    def method(self):\n"
        "        pass\n"
        "\n"
        "def standalone():\n"
        "    return 1\n"
    )
    root = _parse(src)
    decls = extract_declarations(root)
    assert len(decls) >= 2
    kinds = {getattr(d, "kind", None) for d in decls}
    assert kinds <= {"FunctionDecl", "MethodDecl"}


def test_decl_coverage_vacuous_empty_put():
    # Source with no function declarations
    put = _parse("x = 1\ny = x + 2\n")
    test_src = _parse("def test_something(): pass\n")
    rep = declaration_coverage([put], [test_src], k=3)
    assert rep.put_declaration_count == 0
    assert rep.mean_declaration_coverage == pytest.approx(1.0)
    assert rep.declaration_coverage_rate == pytest.approx(1.0)


def test_decl_coverage_invalid_k_raises():
    put = _parse("def f(): return 0\n")
    with pytest.raises(ValueError, match="k must be"):
        declaration_coverage([put], [put], k=0)


def test_decl_coverage_report_is_frozen_dataclass():
    put = _parse("def f(): return 1\n")
    rep = declaration_coverage([put], [put], k=2)
    assert isinstance(rep, DeclarationCoverageReport)
    with pytest.raises(FrozenInstanceError):
        rep.mean_declaration_coverage = 0.0  # type: ignore[misc]
