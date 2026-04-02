"""
Metrics Module
--------------
Quantitative measures that feed into the Subobject Classifier.

Each metric represents a different 'axis' of code quality:
- Complexity: The density of logical paths (branching manifold)
- Distance: Topological drift from canonical forms
- Entropy: Algorithmic compressibility (Kolmogorov proxy)

These metrics are combined by Ω to produce evaluation values in the lattice.
"""

from topos.metrics.complexity import calculate_cyclomatic_complexity
from topos.metrics.distance import calculate_ast_distance
from topos.metrics.entropy import calculate_kolmogorov_proxy

__all__ = [
    "calculate_cyclomatic_complexity",
    "calculate_ast_distance",
    "calculate_kolmogorov_proxy",
]
