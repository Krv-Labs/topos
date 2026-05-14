"""
Module Dependency Graph (MDG) representation.

Inter-module import / call / inheritance graph parsed from GitNexus output.
Feeds the COMPOSABLE generator of H(G_qual) via the Martin coupling and
instability probes in :mod:`topos.functors.probes.mdg`.

Compare with :mod:`topos.graphs.pdg`, which holds the academic
**intra-procedural Program Dependence Graph** (control + data dependence
within a single procedure).  The two are independent representations;
the MDG operates at module granularity while the PDG operates at
statement granularity.
"""

from topos.graphs.mdg.object import (
    GraphNode,
    GraphRelationship,
    ModuleDependencyGraph,
)

__all__ = [
    "ModuleDependencyGraph",
    "GraphNode",
    "GraphRelationship",
]
