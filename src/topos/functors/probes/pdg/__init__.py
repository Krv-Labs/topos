"""
Dependency Graph Metrics Sub-package
-------------------------------------
Metrics that operate on the :class:`~topos.graphs.depgraph.DependencyGraph`
representation:

- Coupling (afferent/efferent) and instability
- Fan-in / fan-out
- Dependency depth
"""

from topos.functors.probes.pdg.coupling import (
    calculate_coupling,
    calculate_dependency_depth,
    calculate_instability,
)
from topos.functors.probes.pdg.fan import calculate_fan_in_out

__all__ = [
    "calculate_coupling",
    "calculate_dependency_depth",
    "calculate_instability",
    "calculate_fan_in_out",
]
