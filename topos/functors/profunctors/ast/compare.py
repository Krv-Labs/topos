"""
Distance Module
---------------
Provides two structural distance metrics between program ASTs.

**1. Tree Edit Distance (TED) — ``calculate_ast_distance``**

    Mathematical Inspiration:
        In topology, distance measures how 'far apart' two points are in a
        space. For programs, we define distance over their AST structures—two
        programs are 'close' if their syntax trees are similar.

        This metric is useful for:
        1. Detecting code clones (near-zero distance)
        2. Measuring refactoring impact (how much structure changed)
        3. Comparing LLM outputs to reference implementations

    We implement a Tree Edit Distance (TED) algorithm that counts the minimum
    number of node insertions, deletions, and relabelings needed to transform
    one tree into another. The implementation uses the Wagner-Fischer
    algorithm on node-type sequences (DFS order), which is an approximation
    of true structural TED optimized for speed.

**2. Gromov-Wasserstein Distance (GHW) — ``calculate_ghw_distance``**

    Mathematical Inspiration:
        The Gromov-Hausdorff distance measures how far two metric spaces are
        from being isometric — that is, how much distortion is needed to
        embed one space into the other. The Wasserstein (optimal transport)
        variant equips each space with a probability measure and finds the
        coupling that minimizes expected pairwise distance distortion.

        For ASTs this captures structural topology that TED misses: two
        programs with identical node-type sequences but different tree shapes
        will have low TED but high GHW distance.

    Algorithm (Frank-Wolfe with Sinkhorn projection):
        1. Model each AST as a metric measure space (X, d_X, μ_X):
           - X = set of AST nodes
           - d_X(u, v) = number of edges on the unique tree path u → v
           - μ_X = uniform probability measure over X
        2. Find the coupling T ∈ Π(μ_X, μ_Y) minimizing the GW cost:
               GW = Σ_{i,j,k,l} (d_X(i,k) − d_Y(j,l))² T[i,j] T[k,l]
        3. Iterate: compute gradient M, Sinkhorn-project to update T.
        4. Normalize by the sum of within-tree second moments to yield [0, 1].

    Potential speedups (not yet implemented):
        - **Linearized GW** (node eccentricity signatures): represent each
          node by its sorted distance vector to all others, build cost matrix
          C[i,j] = ‖sig(u) − sig(v)‖, run a single Sinkhorn pass. ~10×
          faster; no outer loop needed.
        - **1D Wasserstein on distance distributions**: compute empirical
          distributions of all pairwise tree-path distances, then take the
          1D Wasserstein (sort + mean absolute difference). O(n² log n), no
          coupling matrix; good for large-scale clone screening.
        - **Landmark subsampling**: instead of DFS-prefix subsampling,
          select semantically important nodes (function/class definitions)
          as landmarks, reducing n without losing structural signal.
        - **Depth-based LCA approximation**: replace full BFS distance
          matrices with d(u,v) ≈ depth(u) + depth(v) − 2·depth(LCA(u,v))
          via a sparse LCA structure, eliminating the O(n²) BFS pass.
"""

from __future__ import annotations

from topos.topos_functors import DistanceResult, GHWDistanceResult
from topos.topos_functors import calculate_ast_distance as calculate_ast_distance
from topos.topos_functors import calculate_ghw_distance as calculate_ghw_distance
from topos.topos_functors import calculate_similarity as calculate_similarity
from topos.topos_functors import (
    compute_sequence_distance as _compute_sequence_distance,  # noqa: F401 (re-exported for topos.functors.{probes,profunctors}.uast.compare)
)
from topos.topos_functors import structural_distance as structural_distance

__all__ = [
    "DistanceResult",
    "GHWDistanceResult",
    "calculate_ast_distance",
    "calculate_ghw_distance",
    "calculate_similarity",
    "structural_distance",
]
