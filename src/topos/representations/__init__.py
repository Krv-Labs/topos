"""
Representations Package
-----------------------
Program representations: distinct structural views of the same code.

Each sub-package (``ast``, ``depgraph``, ...) defines a concrete
representation that conforms to the :class:`Representation` protocol.
"""

from topos.representations.ast.object import ASTRepresentation
from topos.representations.base import Representation
from topos.representations.depgraph.graph import DependencyGraph

__all__ = [
    "Representation",
    "ASTRepresentation",
    "DependencyGraph",
]
