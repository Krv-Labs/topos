"""
Clone detection policy (pairwise, outside Ω).
---------------------------------------------

Functors compute normalized AST distance; this module applies the
clone threshold.  Not a ``Φᵢ`` translator — does not participate in the
SIMPLE / COMPOSABLE / SECURE lattice.
"""

from __future__ import annotations

from typing import TYPE_CHECKING

from topos.functors.profunctors.ast.compare import calculate_ast_distance

if TYPE_CHECKING:
    from topos.core.object import ProgramObject


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
