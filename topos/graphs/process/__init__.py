"""
Process-flow representation.

Lifts GitNexus execution flows (``Process`` nodes + ``STEP_IN_PROCESS`` edges)
into a :class:`~topos.graphs.base.Representation`. This is the interprocedural
complement to the intra-procedural CFG/CPG and the module-level
:class:`~topos.graphs.mdg.object.ModuleDependencyGraph`: it measures the
*sequences* a program executes rather than the structure of any single unit.
"""

from topos.graphs.process.object import ProcessFlow, ProcessFlowGraph

__all__ = [
    "ProcessFlow",
    "ProcessFlowGraph",
]
