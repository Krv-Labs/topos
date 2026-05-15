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
