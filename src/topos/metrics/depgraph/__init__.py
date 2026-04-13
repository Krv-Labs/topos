"""
Dependency Graph Metrics Sub-package
-------------------------------------
Metrics that operate on the :class:`~topos.representations.depgraph.DependencyGraph`
representation:

- Coupling (afferent/efferent) and instability
- Fan-in / fan-out
- Dependency depth
"""

from topos.metrics.depgraph.coupling import (
    calculate_coupling,
    calculate_dependency_depth,
    calculate_instability,
)
from topos.metrics.depgraph.fan import calculate_fan_in_out

__all__ = [
    "calculate_coupling",
    "calculate_dependency_depth",
    "calculate_instability",
    "calculate_fan_in_out",
]
