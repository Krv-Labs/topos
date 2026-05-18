"""
Core Module
-----------
Defines the fundamental categorical structures of the program topos.

The program topos ``E = Set^(C × H^op)`` decomposes in this codebase as
follows:

    :class:`ProgramObject`
        The objects of the base category ``C`` — Abstract Syntax Trees,
        the *shape* of code.

    :class:`ProgramMorphism`
        A morphism in ``E`` — a program viewed as a transformation
        between computational states.

    :class:`Omega`
        ``Ω`` — the subobject classifier and the value Heyting algebra
        of the topos's internal logic.  A defining part of the topos,
        not of the evaluation pipeline.

    :class:`ProgramCategory`
        The category itself: identity construction, composition, and a
        handle on Ω.

How programs get *mapped* into Ω (the characteristic morphism χ_S and
the policy translators Φᵢ) lives in :mod:`topos.evaluation`; the
translational functors that lift source code into ``E`` (AST, CFG,
UAST, MDG, PDG, CPG) live in :mod:`topos.graphs`.  Selected names from
those packages are re-exported here for convenience.
"""

from topos.core.category import ProgramCategory
from topos.core.morphism import ProgramMorphism
from topos.core.object import ProgramObject
from topos.core.omega import (
    EvaluationValue,
    Omega,
    verdict_from_generators,
)
from topos.graphs.ast.object import ASTRepresentation
from topos.graphs.base import Representation
from topos.graphs.mdg.object import ModuleDependencyGraph

__all__ = [
    # Categorical primitives
    "ProgramCategory",
    "ProgramMorphism",
    "ProgramObject",
    # Subobject classifier (defining part of the topos)
    "Omega",
    "EvaluationValue",
    "verdict_from_generators",
    # Representations re-exported for convenience
    "Representation",
    "ASTRepresentation",
    "ModuleDependencyGraph",
]
