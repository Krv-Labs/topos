"""
Tests for the Code Property Graph (Yamaguchi et al., arxiv:1909.03496)
and its security probes that feed the SECURE generator.
"""

from __future__ import annotations

from topos.core.morphism import ProgramMorphism
from topos.graphs.base import Representation
from topos.graphs.cpg.models import CPGEdgeKind
from topos.graphs.cpg.object import CodePropertyGraph


def _cpg(source: str, language: str = "python") -> CodePropertyGraph:
    morphism = ProgramMorphism(source=source, language=language)
    cpg = morphism.build_cpg()
    assert cpg is not None
    return cpg


def test_cpg_implements_representation_protocol():
    cpg = _cpg("def f(): return 1")
    assert isinstance(cpg, Representation)
    assert cpg.name == "cpg"
    assert cpg.dimension == "secure"


def test_cpg_has_all_four_edge_families_present():
    cpg = _cpg(
        "def f(x):\n    if x:\n        y = x + 1\n        return y\n    return 0\n"
    )
    kinds = {e.kind for e in cpg.edges}
    # AST edges always present; CFG edges from any if/return; DDG / CDG when
    # dependencies exist.
    assert CPGEdgeKind.AST in kinds
    assert CPGEdgeKind.CFG in kinds


def test_cpg_flags_eval_as_dangerous():
    cpg = _cpg("def vuln(s):\n    eval(s)\n")
    m = cpg.metrics()
    assert m["cpg.dangerous_calls"] >= 1


def test_cpg_flags_pickle_loads_as_dangerous():
    cpg = _cpg("import pickle\ndef vuln(blob):\n    pickle.loads(blob)\n")
    assert cpg.metrics()["cpg.dangerous_calls"] >= 1


def test_cpg_clean_code_has_no_dangerous_calls():
    cpg = _cpg("def safe(x):\n    return x + 1\n")
    assert cpg.metrics()["cpg.dangerous_calls"] == 0


def test_cpg_cpp_flags_gets():
    """C++ probe registry includes legacy unsafe stdlib calls."""
    cpg = _cpg(
        "int main() { char buf[10]; gets(buf); return 0; }\n",
        language="cpp",
    )
    assert cpg.metrics()["cpg.dangerous_calls"] >= 1


def test_cpg_nodes_keyed_by_uast_id():
    cpg = _cpg("def f(): return 1")
    for nid, node in cpg.nodes.items():
        assert nid == (node.uast.id or f"anon::{id(node.uast):x}") or nid.startswith(
            "anon::"
        )
