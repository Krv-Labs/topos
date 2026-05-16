"""
AST Metrics Sub-package
-----------------------
Metrics that operate on the Abstract Syntax Tree representation:
- Kolmogorov proxy via compression (algorithmic entropy)
"""

from topos.functors.probes.ast.entropy import calculate_kolmogorov_proxy

__all__ = [
    "calculate_kolmogorov_proxy",
]
