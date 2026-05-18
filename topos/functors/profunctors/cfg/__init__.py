"""CFG profunctors — cyclomatic / edge-kind / longest-path deltas."""

from topos.functors.profunctors.cfg.compare import (
    CFGComparison,
    compare_cfg,
    cyclomatic_delta,
    edge_kind_histogram,
    edge_kind_l1_distance,
    longest_path_delta,
)

__all__ = [
    "CFGComparison",
    "compare_cfg",
    "cyclomatic_delta",
    "edge_kind_histogram",
    "edge_kind_l1_distance",
    "longest_path_delta",
]
