"""
Detailed inspection of a code string — every metric exposed, function table,
entropy breakdown.
"""

from __future__ import annotations

from topos.core.morphism import ProgramMorphism
from topos.evaluation.policies.base import Priority
from topos.functors.probes.ast.complexity import calculate_function_complexities
from topos.functors.probes.ast.entropy import calculate_entropy_detailed

from ..evaluation import classify_code_string
from ..formatting import to_evaluation_result
from ..schemas import (
    EvaluationResult,
    InspectCodeInput,
    InspectionResult,
    LatticeElement,
)
from ..server import mcp

_READ_ONLY_ANN = {
    "title": "Topos Detailed Inspection",
    "readOnlyHint": True,
    "destructiveHint": False,
    "idempotentHint": True,
    "openWorldHint": False,
}


@mcp.tool(
    name="topos_inspect_code",
    tags={"inspect"},
    annotations=_READ_ONLY_ANN,
)
def topos_inspect_code(params: InspectCodeInput) -> InspectionResult:
    """Full metric breakdown for a code string.

    Returns the lattice evaluation, a *top-N* function complexity table
    (sorted descending; configurable via ``top_n_functions``), and entropy
    details. The top-N cap prevents large files from dumping hundreds of
    functions and blowing out agent context.
    """
    try:
        result = classify_code_string(params.code, params.language, Priority.SIMPLE)
    except Exception as exc:
        empty = EvaluationResult(
            is_parseable=False,
            lattice_element=LatticeElement.SLOP,
            lattice_symbol="⊥",
            lattice_description="evaluation failed",
            dimensions={},
            scores={},
            priority=Priority.SIMPLE,
            guidance="",
            coupling_available=False,
            error=str(exc),
        )
        return InspectionResult(evaluation=empty, total_functions=0, error=str(exc))

    evaluation = to_evaluation_result(result, coupling_available=False)

    morphism = ProgramMorphism(source=params.code, language=params.language)
    all_funcs: dict[str, int] = {}
    if morphism.ast:
        all_funcs = dict(calculate_function_complexities(morphism.ast) or {})
    top_funcs = dict(
        sorted(all_funcs.items(), key=lambda kv: -kv[1])[: params.top_n_functions]
    )

    entropy_details = calculate_entropy_detailed(morphism.source)

    return InspectionResult(
        evaluation=evaluation,
        functions=top_funcs,
        total_functions=len(all_funcs),
        entropy_compression_ratio=entropy_details.ratio,
        entropy_interpretation=entropy_details.interpretation,
    )
