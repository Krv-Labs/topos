"""
Tests for the per-representation profunctors D : E × E^op → ℝ.

Each profunctor takes two program morphisms (via the relevant
Representation) and returns a structured delta / divergence.
"""

from __future__ import annotations

import pytest

from topos.core.morphism import ProgramMorphism
from topos.functors.profunctors.cfg import compare_cfg, cyclomatic_delta
from topos.functors.profunctors.cpg import compare_cpg
from topos.functors.profunctors.pdg import compare_pdg, data_dep_jaccard
from topos.graphs.mdg.object import (
    GraphNode,
    GraphRelationship,
    ModuleDependencyGraph,
)

# ---------------------------------------------------------------------------
# CFG profunctor
# ---------------------------------------------------------------------------


def test_cfg_compare_identical_is_zero():
    src = "def f(x):\n    if x: return 1\n    return 0\n"
    a = ProgramMorphism(source=src, language="python").build_cfg()
    b = ProgramMorphism(source=src, language="python").build_cfg()
    cmp = compare_cfg(a, b)
    assert cmp.cyclomatic_delta == 0
    assert cmp.edge_kind_l1 == 0.0
    assert cmp.longest_path_delta == 0
    assert cmp.changed is False


def test_cfg_compare_detects_added_branch():
    simpler = "def f(x):\n    return 0\n"
    branchy = "def f(x):\n    if x > 0:\n        return 1\n    return 0\n"
    a = ProgramMorphism(source=simpler, language="python").build_cfg()
    b = ProgramMorphism(source=branchy, language="python").build_cfg()
    assert cyclomatic_delta(a, b) > 0
    cmp = compare_cfg(a, b)
    assert cmp.changed
    assert cmp.cyclomatic_delta == cyclomatic_delta(a, b)


# ---------------------------------------------------------------------------
# PDG profunctor
# ---------------------------------------------------------------------------


def test_pdg_compare_identical_is_full_jaccard():
    src = "def f(x):\n    y = x + 1\n    if x > 0:\n        y = y * 2\n    return y\n"
    a = ProgramMorphism(source=src, language="python").build_pdg()
    b = ProgramMorphism(source=src, language="python").build_pdg()
    cmp = compare_pdg(a, b)
    assert cmp.data_dep_jaccard == 1.0
    assert cmp.control_dep_jaccard == 1.0
    assert cmp.statement_delta == 0


def test_pdg_data_dep_jaccard_is_well_defined():
    src_a = "def f(x):\n    return x + 1\n"
    src_b = "def g(y):\n    return y + 2\n"
    a = ProgramMorphism(source=src_a, language="python").build_pdg()
    b = ProgramMorphism(source=src_b, language="python").build_pdg()
    j = data_dep_jaccard(a, b)
    assert 0.0 <= j <= 1.0


def test_pdg_control_dep_jaccard_drops_when_branch_added():
    flat = "def f(x):\n    return x\n"
    branchy = "def f(x):\n    if x > 0:\n        return 1\n    return 0\n"
    a = ProgramMorphism(source=flat, language="python").build_pdg()
    b = ProgramMorphism(source=branchy, language="python").build_pdg()
    cmp = compare_pdg(a, b)
    assert cmp.control_dep_jaccard < 1.0


# ---------------------------------------------------------------------------
# MDG profunctor
# ---------------------------------------------------------------------------


def _empty_mdg(target_file: str) -> ModuleDependencyGraph:
    g = ModuleDependencyGraph(target_file=target_file)
    g.add_node(
        GraphNode(
            id=f"File:{target_file}",
            label="File",
            properties={"filePath": target_file},
        )
    )
    return g


def _mdg_with_outgoing_import(target_file: str) -> ModuleDependencyGraph:
    g = _empty_mdg(target_file)
    g.add_node(
        GraphNode(
            id="File:other.py",
            label="File",
            properties={"filePath": "other.py"},
        )
    )
    g.add_relationship(
        GraphRelationship(
            id="i1",
            source_id=f"File:{target_file}",
            target_id="File:other.py",
            type="IMPORTS",
        )
    )
    return g


def test_mdg_compare_identical_isolated():
    from topos.functors.profunctors.mdg import compare_mdg

    a = _empty_mdg("a.py")
    b = _empty_mdg("a.py")
    cmp = compare_mdg(a, b)
    assert cmp.coupling_delta == 0
    assert cmp.fan_in_delta == 0
    assert cmp.fan_out_delta == 0
    assert cmp.changed is False


def test_mdg_compare_detects_added_import_chain():
    from topos.functors.profunctors.mdg import compare_mdg

    a = _empty_mdg("a.py")
    b = _mdg_with_outgoing_import("a.py")
    cmp = compare_mdg(a, b)
    # Either the dep_depth or instability moved.
    assert cmp.dep_depth_delta > 0 or cmp.instability_delta != 0


# ---------------------------------------------------------------------------
# CPG profunctor
# ---------------------------------------------------------------------------


def test_cpg_compare_identical_is_full_jaccard():
    src = "def f(x):\n    return x + 1\n"
    a = ProgramMorphism(source=src, language="python").build_cpg()
    b = ProgramMorphism(source=src, language="python").build_cpg()
    cmp = compare_cpg(a, b)
    assert cmp.node_jaccard == 1.0
    for family, j in cmp.family_jaccards.items():
        assert j == 1.0, f"family {family} jaccard < 1.0"
    assert cmp.dangerous_delta == 0.0
    assert cmp.taint_delta == 0.0
    assert cmp.changed is False


def test_cpg_compare_detects_added_dangerous_api():
    safe = "def f(x):\n    return x + 1\n"
    unsafe = "def f(x):\n    eval(x)\n    return x + 1\n"
    a = ProgramMorphism(source=safe, language="python").build_cpg()
    b = ProgramMorphism(source=unsafe, language="python").build_cpg()
    from topos.functors.profunctors.cpg import dangerous_delta

    assert dangerous_delta(a, b) >= 1.0


@pytest.mark.parametrize(
    "language,src",
    [
        ("python", "def f(): pass\n"),
        ("javascript", "function f() { return 1; }\n"),
    ],
)
def test_cpg_compare_is_symmetric_for_identical(language, src):
    a = ProgramMorphism(source=src, language=language).build_cpg()
    b = ProgramMorphism(source=src, language=language).build_cpg()
    forward = compare_cpg(a, b)
    backward = compare_cpg(b, a)
    assert forward.node_jaccard == backward.node_jaccard
    assert forward.family_jaccards == backward.family_jaccards
