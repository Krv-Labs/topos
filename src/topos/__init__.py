"""
Topos: Category-Theoretic Code Quality Evaluation
==================================================

Treating programs as morphisms in a world of commodity code.
Building the subobject classifier for rigorous program evaluations.

This library applies concepts from Topos theory to evaluate code quality,
moving beyond simple numeric metrics to a Heyting Algebra of evaluation values
that can express partial confidence about program quality and maintainability.
"""

from topos.core.morphism import ProgramMorphism
from topos.core.object import ProgramObject
from topos.graphs.ast.object import ASTRepresentation
from topos.graphs.base import Representation
from topos.graphs.pdg.graph import DependencyGraph
from topos.logic.lattice import (
    EvaluationLattice,
    EvaluationValue,
)
from topos.logic.omega import SubobjectClassifier

__version__ = "0.1.0"

__all__ = [
    "ProgramMorphism",
    "ProgramObject",
    "EvaluationLattice",
    "EvaluationValue",
    "SubobjectClassifier",
    "Representation",
    "ASTRepresentation",
    "DependencyGraph",
]
