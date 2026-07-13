"""
Tests for the academic Program Dependence Graph (intra-procedural,
Ferrante/Ottenstein style).
"""

from __future__ import annotations

from topos.core.morphism import ProgramMorphism
from topos.graphs.base import Representation
from topos.graphs.pdg.object import (
    DependenceKind,
    ProgramDependenceGraph,
)


def _pdg(source: str, language: str = "python") -> ProgramDependenceGraph:
    morphism = ProgramMorphism(source=source, language=language)
    pdg = morphism.build_pdg()
    assert pdg is not None
    return pdg


def test_pdg_implements_representation_protocol():
    pdg = _pdg("def f(): return 1")
    assert isinstance(pdg, Representation)
    assert pdg.name == "pdg"


def test_pdg_metrics_namespace():
    pdg = _pdg("def f(): return 1")
    m = pdg.metrics()
    assert "pdg.data_deps" in m
    assert "pdg.control_deps" in m
    assert "pdg.density" in m


def test_pdg_emits_control_dependence_for_if_statement():
    pdg = _pdg("def f(x):\n    if x > 0:\n        y = 1\n    return y\n")
    control_edges = [e for e in pdg.edges if e.kind is DependenceKind.CONTROL]
    assert len(control_edges) >= 1


def test_pdg_density_is_nonnegative():
    pdg = _pdg("x = 1\ny = x + 2\nz = y * 3\n")
    assert pdg.metrics()["pdg.density"] >= 0.0


def test_pdg_emits_data_dependence_across_statements():
    """A variable defined in one statement and used in a later one must
    produce a DATA dependence edge.

    Regression guard: ``_identifier_name`` used to fall back to each
    identifier occurrence's own node id (unique per byte span) whenever no
    ``name`` attribute was present — which is always, since no UAST mapper
    sets one. Two distinct occurrences of the same variable (e.g. the def
    in ``x = 1`` and the use in ``y = x + 2``) therefore never shared a key,
    so ``_compute_data_dependence`` could never link them and
    ``pdg.data_deps`` was always 0 for real code. Threading ``source``
    through ``ProgramDependenceGraph.from_uast`` lets identifier names be
    recovered from the actual token text, fixing this.
    """
    pdg = _pdg("x = 1\ny = x + 2\n")
    data_edges = [e for e in pdg.edges if e.kind is DependenceKind.DATA]
    assert len(data_edges) >= 1
    assert any(e.var == "x" for e in data_edges)
    assert pdg.metrics()["pdg.data_deps"] >= 1.0


def test_pdg_data_dependence_without_source_falls_back_gracefully():
    """Building a PDG directly from a UAST with no source text must not
    crash, and simply yields no data-dependence edges (the pre-fix
    behavior) rather than spuriously conflating identifiers."""
    from topos.graphs.uast.mapper_python import map_python_tree_to_uast
    from topos.utils.tree_sitter import PythonParser

    root_node = PythonParser().parse("x = 1\ny = x + 2\n")
    uast_root = map_python_tree_to_uast(root_node)

    pdg = ProgramDependenceGraph.from_uast(uast_root)
    data_edges = [e for e in pdg.edges if e.kind is DependenceKind.DATA]
    assert data_edges == []
