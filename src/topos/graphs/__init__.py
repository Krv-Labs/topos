"""
Graphs Package
--------------
Program representations: distinct structural views of the same code.

Each sub-package (``ast``, ``depgraph``, ...) defines a concrete
representation that conforms to the :class:`Representation` protocol.
"""

from topos.graphs.ast.object import ASTRepresentation
from topos.graphs.base import Representation
from topos.graphs.pdg.graph import DependencyGraph

__all__ = [
    "Representation",
    "ASTRepresentation",
    "DependencyGraph",
]
