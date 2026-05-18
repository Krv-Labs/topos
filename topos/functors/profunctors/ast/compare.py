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

from collections import deque
from dataclasses import dataclass
from typing import TYPE_CHECKING, Any

import numpy as np

if TYPE_CHECKING:
    from topos.core.morphism import ProgramMorphism
    from topos.core.object import ProgramObject


# ---------------------------------------------------------------------------
# Tree Edit Distance
# ---------------------------------------------------------------------------


@dataclass
class DistanceResult:
    """
    The result of computing AST edit distance.

    Attributes:
        raw_distance: The absolute edit distance (number of operations).
        normalized_distance: Distance normalized by tree sizes (0-1).
        operations: Breakdown of edit operations.
    """

    raw_distance: int
    normalized_distance: float
    operations: dict[str, int]

    def __str__(self) -> str:
        return (
            f"Distance: {self.raw_distance} "
            f"(normalized: {self.normalized_distance:.3f})"
        )


def calculate_ast_distance(
    source: ProgramObject,
    target: ProgramObject,
) -> DistanceResult:
    """
    Compute the tree edit distance between two program ASTs.

    Uses a simplified tree edit distance algorithm based on
    node type comparison and structural alignment.

    Args:
        source: The source ProgramObject.
        target: The target ProgramObject.

    Returns:
        A DistanceResult containing raw and normalized distances.

    Note:
        This is an approximation of true tree edit distance,
        optimized for speed over exactness. For small trees,
        it's quite accurate; for large trees, it provides a
        reasonable upper bound.
    """
    source_nodes = list(source.traverse())
    target_nodes = list(target.traverse())

    source_types = [n.type for n in source_nodes]
    target_types = [n.type for n in target_nodes]

    distance, ops = _compute_sequence_distance(source_types, target_types)

    max_size = max(len(source_nodes), len(target_nodes), 1)
    normalized = distance / max_size

    return DistanceResult(
        raw_distance=distance,
        normalized_distance=min(normalized, 1.0),
        operations=ops,
    )


def _compute_sequence_distance(
    source: list[str],
    target: list[str],
) -> tuple[int, dict[str, int]]:
    """
    Compute edit distance between two sequences of node types.

    Uses the Wagner-Fischer algorithm (dynamic programming).

    Args:
        source: List of node types from source tree.
        target: List of node types from target tree.

    Returns:
        Tuple of (distance, operation_counts).
    """
    from topos.topos_functors import compute_sequence_distance as rust_calc
    return rust_calc(source, target)


def calculate_similarity(
    source: ProgramObject,
    target: ProgramObject,
) -> float:
    """
    Compute structural similarity between two programs.

    Similarity = 1 - normalized_distance

    Args:
        source: The source ProgramObject.
        target: The target ProgramObject.

    Returns:
        A similarity score in [0, 1], where 1 means identical structure.
    """
    result = calculate_ast_distance(source, target)
    return 1.0 - result.normalized_distance


def structural_distance(source: ProgramMorphism, target: ProgramMorphism) -> float:
    """
    Normalized AST edit distance between two program morphisms.

    Convenience wrapper around :func:`calculate_ast_distance`: extracts
    the AST from each morphism and returns the normalized result in
    ``[0, 1]``.  Returns ``1.0`` if either morphism is unparseable.
    """
    if source.ast is None or target.ast is None:
        return 1.0
    return calculate_ast_distance(source.ast, target.ast).normalized_distance


# ---------------------------------------------------------------------------
# Gromov-Wasserstein Tree Distance
# ---------------------------------------------------------------------------


@dataclass
class GHWDistanceResult:
    """
    The result of computing Gromov-Wasserstein distance between two ASTs.

    Attributes:
        gw_distance: Normalized GW cost in [0, 1]. Zero means the trees are
            isometric under the uniform measure; one means no structural
            correspondence was found.
        raw_gw_cost: Unnormalized GW cost, useful for comparisons at a fixed
            scale (e.g. when both trees have the same number of nodes).
        n_nodes_source: Number of nodes used from the source tree (after
            any subsampling).
        n_nodes_target: Number of nodes used from the target tree.
        n_iterations: Number of outer GW iterations executed.
        converged: True if the coupling change fell below ``tol`` before
            ``n_iter`` was exhausted.
    """

    gw_distance: float
    raw_gw_cost: float
    n_nodes_source: int
    n_nodes_target: int
    n_iterations: int
    converged: bool

    def __str__(self) -> str:
        status = "converged" if self.converged else "max_iter"
        return (
            f"GHW Distance: {self.gw_distance:.4f} "
            f"(raw: {self.raw_gw_cost:.4f}, {status} in {self.n_iterations} iter)"
        )


def _tree_path_distances(nodes: list[Any]) -> np.ndarray:
    """
    Build the pairwise tree-path distance matrix for a list of AST nodes.

    Constructs a bidirectional adjacency list from parent→child edges (using
    Python object identity to key nodes), then runs BFS from every node to
    fill an (n×n) integer distance matrix.

    Args:
        nodes: Ordered list of tree-sitter Node objects (e.g. from
            ``ProgramObject.traverse()``). Only edges whose endpoints both
            appear in this list are included, so subsampled lists work
            correctly.

    Returns:
        Float64 ndarray of shape (n, n) containing hop counts.
    """
    n = len(nodes)
    id_to_idx: dict[int, int] = {id(node): i for i, node in enumerate(nodes)}

    adj: list[list[int]] = [[] for _ in range(n)]
    for i, node in enumerate(nodes):
        for child in node.children:
            j = id_to_idx.get(id(child))
            if j is not None:
                adj[i].append(j)
                adj[j].append(i)

    D = np.full((n, n), fill_value=n + 1, dtype=np.int32)
    np.fill_diagonal(D, 0)

    for start in range(n):
        visited = [False] * n
        visited[start] = True
        queue: deque[int] = deque([start])
        while queue:
            u = queue.popleft()
            for v in adj[u]:
                if not visited[v]:
                    visited[v] = True
                    D[start, v] = D[start, u] + 1
                    queue.append(v)

    return D.astype(np.float64)


def _sinkhorn(
    K: np.ndarray,
    mu: np.ndarray,
    nu: np.ndarray,
    n_iter: int = 100,
) -> np.ndarray:
    """
    Sinkhorn-Knopp scaling to project a Gibbs kernel into Π(μ, ν).

    Iterates row and column normalization until the doubly-stochastic
    constraint (row marginals = μ, column marginals = ν) is satisfied.

    Args:
        K: (n, m) non-negative kernel matrix (e.g. exp(-M/reg)).
        mu: Source marginal of length n.
        nu: Target marginal of length m.
        n_iter: Number of scaling iterations.

    Returns:
        Transport plan T of shape (n, m) in Π(μ, ν).
    """
    _EPS = 1e-300
    u = np.ones(len(mu))
    for _ in range(n_iter):
        Ktu = K.T @ u
        v = nu / np.maximum(Ktu, _EPS)
        Kv = K @ v
        u = mu / np.maximum(Kv, _EPS)
    v = nu / np.maximum(K.T @ u, _EPS)
    return u[:, None] * K * v[None, :]


def _gromov_wasserstein(
    D1: np.ndarray,
    D2: np.ndarray,
    n_iter: int,
    reg: float,
    tol: float,
) -> tuple[np.ndarray, int, bool]:
    """
    Frank-Wolfe iterations for the entropic Gromov-Wasserstein problem.

    At each step linearizes the GW cost around the current coupling T,
    then uses Sinkhorn to solve the resulting linear transport subproblem.

    GW gradient (w.r.t. T, given marginals μ, ν):
        M[i,j] = 2 * ((D1²@μ)[i] + (D2²@ν)[j] − 2*(D1@T@D2)[i,j])

    The Gibbs kernel uses a shift for numerical stability:
        K = exp(−(M − min(M)) / reg)
    Since Sinkhorn is scale-invariant, this is equivalent to the unshifted
    kernel but avoids float64 underflow when M entries are large relative
    to reg.

    Args:
        D1: (n, n) distance matrix for source tree.
        D2: (m, m) distance matrix for target tree.
        n_iter: Maximum number of outer GW iterations.
        reg: Sinkhorn entropy regularization coefficient.
        tol: Convergence tolerance on the Frobenius norm of coupling change.

    Returns:
        Tuple of (optimal_T, iterations_run, converged).
    """
    n = len(D1)
    m = len(D2)
    mu = np.ones(n) / n
    nu = np.ones(m) / m

    T = np.outer(mu, nu)

    D1sq_mu = (D1**2) @ mu  # (n,) — constant across iterations
    D2sq_nu = (D2**2) @ nu  # (m,) — constant across iterations

    converged = False
    iterations_run = 0
    for _it in range(1, n_iter + 1):
        iterations_run = _it
        M = 2.0 * (D1sq_mu[:, None] + D2sq_nu[None, :] - 2.0 * (D1 @ T @ D2))
        # Shift by minimum for numerical stability (Sinkhorn is shift-invariant)
        K = np.exp(-(M - M.min()) / reg)
        T_new = _sinkhorn(K, mu, nu)
        delta = np.linalg.norm(T_new - T)
        T = T_new
        if delta < tol:
            converged = True
            break

    return T, iterations_run, converged


def calculate_ghw_distance(
    source: ProgramObject,
    target: ProgramObject,
    max_nodes: int = 200,
    n_iter: int = 50,
    reg: float = 0.5,
    tol: float = 1e-6,
    return_coupling: bool = False,
) -> GHWDistanceResult:
    """
    Compute the Gromov-Wasserstein distance between two program ASTs.

    Models each AST as a metric measure space: nodes as points, tree-path
    length (edge hops) as the metric, and a uniform probability measure over
    nodes. Finds the optimal coupling T ∈ Π(μ, ν) that minimizes the GW cost:

        GW = Σ_{i,j,k,l} (d_X(i,k) − d_Y(j,l))² T[i,j] T[k,l]

    via a Frank-Wolfe loop with entropic (Sinkhorn) projection.

    **When to use this over** ``calculate_ast_distance``:
    The TED implementation compares sequences of node types and ignores tree
    topology. Two programs with identical node-type multisets but different
    nesting structure will score near-zero TED but high GHW distance. Use GHW
    when structural shape — depth, branching, subtree distribution — matters.

    **Normalization**:
        gw_distance = raw_gw_cost / (μᵀD1²μ + νᵀD2²ν)

    The denominator is the maximum possible GW cost (when the cross-term
    vanishes), so ``gw_distance`` lies in [0, 1] with 0 meaning the trees
    are metrically isometric under the uniform measure.

    Args:
        source: The source ProgramObject.
        target: The target ProgramObject.
        max_nodes: Subsampling cap. If a tree has more than ``max_nodes``
            nodes, the first ``max_nodes`` in DFS order are used.
            Structural (non-leaf) nodes appear early in DFS, so this retains
            the most topologically informative portion of the tree.
        n_iter: Maximum outer GW iterations (Frank-Wolfe steps).
        reg: Sinkhorn entropy regularization. Smaller values yield sharper
            couplings but may require more Sinkhorn iterations to converge.
        tol: Convergence tolerance on ‖T_new − T‖_F. Iteration stops early
            when the coupling update is below this threshold.
        return_coupling: If True, the optimal coupling matrix T (shape
            n×m, dtype float64) is attached as ``result.coupling``. Off by
            default; only needed for inspection or visualization.

    Returns:
        GHWDistanceResult with ``gw_distance`` in [0, 1].

    Performance note:
        This is the full GW formulation: O(n²) for distance matrices via BFS,
        and O(n²m + nm²) per Frank-Wolfe iteration. For large codebases or
        latency-sensitive pipelines, consider these faster approximations
        (not yet implemented):

        - **Linearized GW** (node eccentricity signatures): represent each
          node by its sorted distance vector to all others and build cost
          matrix C[i,j] = ‖sig(u) − sig(v)‖. A single Sinkhorn pass then
          gives an approximate coupling without an outer loop (~10× faster).
        - **1D Wasserstein on distance distributions**: collect all pairwise
          tree-path distances as an empirical distribution per tree and
          compute their 1D Wasserstein distance (sort + mean absolute
          difference). O(n² log n), no coupling matrix; suitable for
          large-scale clone screening.
        - **Landmark subsampling**: replace DFS-prefix subsampling with
          semantically motivated landmarks (e.g. function / class definition
          nodes), reducing n while preserving structural signal.
        - **Depth-based LCA approximation**: avoid the O(n²) full BFS pass
          by using d(u,v) ≈ depth(u) + depth(v) − 2·depth(LCA(u,v)) via a
          sparse LCA structure.
    """
    source_nodes = list(source.traverse())
    target_nodes = list(target.traverse())

    if len(source_nodes) > max_nodes:
        source_nodes = source_nodes[:max_nodes]
    if len(target_nodes) > max_nodes:
        target_nodes = target_nodes[:max_nodes]

    D1 = _tree_path_distances(source_nodes)
    D2 = _tree_path_distances(target_nodes)

    T, n_iterations, converged = _gromov_wasserstein(D1, D2, n_iter, reg, tol)

    mu = np.ones(len(source_nodes)) / len(source_nodes)
    nu = np.ones(len(target_nodes)) / len(target_nodes)

    # GW cost via trace identity — avoids building the O(n²m²) tensor:
    #   GW = μᵀD1²μ + νᵀD2²ν − 2·tr(D2·TᵀD1T)
    cross = float(np.sum(D2 * (T.T @ D1 @ T)))
    self_term = float(mu @ (D1**2) @ mu + nu @ (D2**2) @ nu)
    raw_gw_cost = self_term - 2.0 * cross

    gw_distance = raw_gw_cost / self_term if self_term > 0.0 else 0.0
    gw_distance = float(np.clip(gw_distance, 0.0, 1.0))

    result = GHWDistanceResult(
        gw_distance=gw_distance,
        raw_gw_cost=raw_gw_cost,
        n_nodes_source=len(source_nodes),
        n_nodes_target=len(target_nodes),
        n_iterations=n_iterations,
        converged=converged,
    )

    if return_coupling:
        result.coupling = T  # type: ignore[attr-defined]

    return result
