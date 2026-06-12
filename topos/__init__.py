"""
Topos: Category-Theoretic Code Quality Evaluation
==================================================

Treating programs as morphisms in a world of commodity code.
Building the subobject classifier for rigorous program evaluations.

This library applies concepts from Topos theory to evaluate code quality,
moving beyond simple numeric metrics to a Heyting Algebra of evaluation
values that can express partial confidence about program quality and
maintainability.
"""

from topos.core.category import ProgramCategory
from topos.core.morphism import ProgramMorphism
from topos.core.object import ProgramObject
from topos.core.omega import (
    EvaluationValue,
    Omega,
    verdict_from_generators,
)
from topos.evaluation.characteristic_morphism import (
    CharacteristicMorphism,
    ClassificationResult,
)
from topos.graphs.ast.object import ASTRepresentation
from topos.graphs.base import Representation
from topos.graphs.cfg.object import ControlFlowGraph
from topos.graphs.cpg.object import CodePropertyGraph
from topos.graphs.mdg.object import ModuleDependencyGraph
from topos.graphs.pdg.object import ProgramDependenceGraph

__version__ = "0.3.4"

__all__ = [
    # Categorical primitives
    "ProgramCategory",
    "ProgramMorphism",
    "ProgramObject",
    # Internal logic of the topos
    "Omega",
    "EvaluationValue",
    "verdict_from_generators",
    "CharacteristicMorphism",
    "ClassificationResult",
    # Translational functors (representations)
    "Representation",
    "ASTRepresentation",
    "ControlFlowGraph",
    "ProgramDependenceGraph",
    "ModuleDependencyGraph",
    "CodePropertyGraph",
]
