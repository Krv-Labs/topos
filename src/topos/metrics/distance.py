"""
Distance Module
---------------
Computes the 'Topological Drift' between program morphisms.

Mathematical Inspiration:
    In topology, distance measures how 'far apart' two points are in a space.
    For programs, we define distance over their AST structures—two programs
    are 'close' if their syntax trees are similar.

    This metric is crucial for:
    1. Detecting code clones (near-zero distance)
    2. Measuring refactoring impact (how much structure changed)
    3. Comparing LLM outputs to reference implementations

    We implement a Tree Edit Distance (TED) algorithm, which counts the
    minimum number of node insertions, deletions, and relabelings needed
    to transform one tree into another.

    The Zhang-Shasha algorithm provides an O(n²) solution for ordered trees,
    which we simplify here for practical use.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from topos.core.object import ProgramObject


@dataclass
class DistanceResult:
    """
    The result of computing AST distance.

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
    m, n = len(source), len(target)

    dp = [[0] * (n + 1) for _ in range(m + 1)]

    for i in range(m + 1):
        dp[i][0] = i
    for j in range(n + 1):
        dp[0][j] = j

    for i in range(1, m + 1):
        for j in range(1, n + 1):
            if source[i - 1] == target[j - 1]:
                dp[i][j] = dp[i - 1][j - 1]
            else:
                dp[i][j] = 1 + min(
                    dp[i - 1][j],  # deletion
                    dp[i][j - 1],  # insertion
                    dp[i - 1][j - 1],  # substitution
                )

    insertions = 0
    deletions = 0
    substitutions = 0

    i, j = m, n
    while i > 0 or j > 0:
        if i > 0 and j > 0 and source[i - 1] == target[j - 1]:
            i -= 1
            j -= 1
        elif i > 0 and j > 0 and dp[i][j] == dp[i - 1][j - 1] + 1:
            substitutions += 1
            i -= 1
            j -= 1
        elif j > 0 and dp[i][j] == dp[i][j - 1] + 1:
            insertions += 1
            j -= 1
        elif i > 0:
            deletions += 1
            i -= 1
        else:
            break

    return dp[m][n], {
        "insertions": insertions,
        "deletions": deletions,
        "substitutions": substitutions,
    }


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


def are_clones(
    source: ProgramObject,
    target: ProgramObject,
    threshold: float = 0.1,
) -> bool:
    """
    Check if two programs are structural clones.

    Programs are considered clones if their normalized distance
    is below the threshold.

    Args:
        source: The source ProgramObject.
        target: The target ProgramObject.
        threshold: Maximum normalized distance for clone detection.

    Returns:
        True if the programs are clones.
    """
    result = calculate_ast_distance(source, target)
    return result.normalized_distance <= threshold
