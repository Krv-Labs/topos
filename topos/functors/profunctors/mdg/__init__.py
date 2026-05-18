"""MDG profunctors — coupling / instability / fan-in-out / dep-depth deltas."""

from topos.functors.profunctors.mdg.compare import (
    MDGComparison,
    compare_mdg,
    coupling_delta,
    dep_depth_delta,
    fan_in_delta,
    fan_out_delta,
    instability_delta,
)

__all__ = [
    "MDGComparison",
    "compare_mdg",
    "coupling_delta",
    "dep_depth_delta",
    "fan_in_delta",
    "fan_out_delta",
    "instability_delta",
]
