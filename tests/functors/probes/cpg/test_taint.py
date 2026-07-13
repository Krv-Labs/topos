"""
Regression tests for ``taint_flow_paths`` (issue #154).

DDG edges connect whole *statement* nodes (see
``ProgramDependenceGraph._compute_data_dependence``), while source/sink
detection matches individual *sub-expression* nodes (a bare
``Identifier``/``MemberExpr`` for sources, a ``CallExpr`` for sinks). These
two id spaces essentially never intersect directly, so ``taint_flow_paths``
used to return 0 even for the textbook direct-flow case. It now bridges the
gap by containment: a source/sink is "connected" if it is a descendant of
(or equal to) a DDG-participating statement.
"""

from __future__ import annotations

from topos.core.morphism import ProgramMorphism
from topos.functors.probes.cpg.taint import taint_flow_paths
from topos.graphs.cpg.object import CodePropertyGraph


def _cpg(source: str, language: str = "python") -> CodePropertyGraph:
    morphism = ProgramMorphism(source=source, language=language)
    cpg = morphism.build_cpg()
    assert cpg is not None
    return cpg


def test_taint_flow_paths_direct_python_source_to_sink():
    """Issue #154's exact repro: `x = input(); eval(x)` must count >= 1."""
    cpg = _cpg("def f():\n    x = input()\n    eval(x)\n")
    assert taint_flow_paths(cpg) >= 1


def test_taint_flow_paths_direct_go_source_to_sink():
    """Same granularity-bridging behavior for a non-Python language."""
    source = (
        "package main\n\n"
        "import (\n"
        '\t"os"\n'
        '\t"os/exec"\n'
        ")\n\n"
        "func run() {\n"
        "\tvar v string\n"
        '\tv = os.Getenv("X")\n'
        '\texec.Command("sh", "-c", v)\n'
        "}\n"
    )
    cpg = _cpg(source, language="go")
    assert taint_flow_paths(cpg) >= 1


def test_taint_flow_paths_no_flow_when_source_and_sink_unrelated():
    """A source and a sink with no data-dependence between them stay at 0."""
    cpg = _cpg("def f():\n    x = input()\n    y = 1\n    eval(y)\n")
    assert taint_flow_paths(cpg) == 0


def test_taint_flow_paths_do_not_cross_function_scope_by_name_only():
    cpg = _cpg("def source_func():\n    x = input()\n\ndef sink_func():\n    eval(x)\n")
    assert taint_flow_paths(cpg) == 0


def test_taint_flow_paths_clean_code_has_no_flows():
    cpg = _cpg("def f(a, b):\n    return a + b\n")
    assert taint_flow_paths(cpg) == 0


def test_taint_flow_paths_respects_allowlist():
    """An allowlisted sink pattern is excluded even when a real flow exists."""
    cpg = _cpg("def f():\n    x = input()\n    eval(x)\n")
    assert taint_flow_paths(cpg) >= 1
    assert taint_flow_paths(cpg, allow={"eval"}) == 0
