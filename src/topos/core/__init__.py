"""
Core Module
-----------
Defines the fundamental categorical structures: Objects and Morphisms.

In the category of Programs:
- Objects are Abstract Syntax Trees (the 'shape' of code)
- Morphisms are programs themselves (transformations between states)

Program representations (AST, dependency graph, ...) live in the
``topos.representations`` package and are re-exported here for
convenience.
"""

from topos.core.morphism import ProgramMorphism
from topos.core.object import ProgramObject
from topos.representations.ast.object import ASTRepresentation
from topos.representations.base import Representation
from topos.representations.depgraph.graph import DependencyGraph

__all__ = [
    "ProgramMorphism",
    "ProgramObject",
    "Representation",
    "ASTRepresentation",
    "DependencyGraph",
]
