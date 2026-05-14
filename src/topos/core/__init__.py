"""
Core Module
-----------
Defines the fundamental categorical structures: Objects and Morphisms.

In the category of Programs:
- Objects are Abstract Syntax Trees (the 'shape' of code)
- Morphisms are programs themselves (transformations between states)

Program representations (AST, dependency graph, ...) live in the
``topos.graphs`` package and are re-exported here for
convenience.
"""

from topos.core.morphism import ProgramMorphism
from topos.core.object import ProgramObject
from topos.graphs.ast.object import ASTRepresentation
from topos.graphs.base import Representation
from topos.graphs.mdg.object import DependencyGraph

__all__ = [
    "ProgramMorphism",
    "ProgramObject",
    "Representation",
    "ASTRepresentation",
    "DependencyGraph",
]
