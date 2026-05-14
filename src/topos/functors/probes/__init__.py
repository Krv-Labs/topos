"""
Metrics Module
--------------
Quantitative measures that feed into the Subobject Classifier.

Metrics are organized by representation type:
- ``metrics.ast`` -- AST-derived metrics (complexity, entropy)
- ``metrics.depgraph`` -- Dependency-graph metrics (coupling, fan-in/out)
- ``metrics.distance`` -- Cross-representation distance (AST edit distance)

For backward compatibility the most common AST metric functions are
re-exported here so that ``from topos.metrics import ...`` keeps working.
"""

from topos.functors.probes.ast.complexity import calculate_cyclomatic_complexity
from topos.functors.probes.ast.entropy import calculate_kolmogorov_proxy
from topos.functors.profunctors.distance import calculate_ast_distance

__all__ = [
    "calculate_cyclomatic_complexity",
    "calculate_ast_distance",
    "calculate_kolmogorov_proxy",
]
