"""
Dependency Graph Representation Sub-package
--------------------------------------------
Parses GitNexus output into a :class:`DependencyGraph` that conforms
to the :class:`~topos.graphs.base.Representation` protocol.
"""

from topos.graphs.depgraph.graph import (
    DependencyGraph,
    GraphNode,
    GraphRelationship,
)

__all__ = ["DependencyGraph", "GraphNode", "GraphRelationship"]
