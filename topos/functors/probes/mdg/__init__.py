"""
Module Dependency Graph (MDG) probes — P : E -> R restricted to the
inter-module dependency-graph functor's image.

Probes consumed by Φ_COMPOSABLE to score the COMPOSABLE generator:

- Coupling (afferent/efferent) and Martin instability
- Fan-in / fan-out
- Dependency depth
"""

from topos.functors.probes.mdg.coupling import (
    calculate_coupling,
    calculate_dependency_depth,
    calculate_instability,
)
from topos.functors.probes.mdg.fan import calculate_fan_in_out

__all__ = [
    "calculate_coupling",
    "calculate_dependency_depth",
    "calculate_instability",
    "calculate_fan_in_out",
]
