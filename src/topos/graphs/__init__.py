"""
Graphs Package
--------------
Program representations: distinct structural views of the same code.

Each sub-package implements one translational functor ``R: Lang -> E`` and
conforms to the :class:`Representation` protocol.

Currently shipped:
    ``ast``  — concrete syntax (per-language tree-sitter parsers)
    ``uast`` — language-independent normalized AST
    ``cfg``  — intra-procedural control flow graph (feeds SIMPLE)
    ``pdg``  — academic Program Dependence Graph (intra-procedural,
               Ferrante/Ottenstein style)
    ``mdg``  — Module Dependency Graph from GitNexus (feeds COMPOSABLE)
    ``cpg``  — Code Property Graph (AST ∪ CFG ∪ DDG ∪ CDG, feeds SECURE)
"""

from topos.graphs.ast.object import ASTRepresentation
from topos.graphs.base import Representation
from topos.graphs.cfg.object import ControlFlowGraph
from topos.graphs.cpg.object import CodePropertyGraph
from topos.graphs.mdg.object import DependencyGraph, ModuleDependencyGraph
from topos.graphs.pdg.object import ProgramDependenceGraph

__all__ = [
    "Representation",
    "ASTRepresentation",
    "ControlFlowGraph",
    "ProgramDependenceGraph",
    "ModuleDependencyGraph",
    "CodePropertyGraph",
    # Deprecated alias for ModuleDependencyGraph.
    "DependencyGraph",
]
