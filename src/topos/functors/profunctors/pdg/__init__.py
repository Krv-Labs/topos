"""PDG profunctors — DDG and CDG edge-set Jaccards, statement / density deltas."""

from topos.functors.profunctors.pdg.compare import (
    PDGComparison,
    compare_pdg,
    control_dep_jaccard,
    data_dep_jaccard,
    density_delta,
    statement_delta,
)

__all__ = [
    "PDGComparison",
    "compare_pdg",
    "control_dep_jaccard",
    "data_dep_jaccard",
    "density_delta",
    "statement_delta",
]
