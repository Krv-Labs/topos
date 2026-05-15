"""
Detailed inspection of a code string — every metric exposed, function table,
entropy breakdown.
"""

from __future__ import annotations

from topos.core.morphism import ProgramMorphism
from topos.evaluation.policies.base import Priority
from topos.functors.probes.cfg.complexity import cyclomatic_complexity
from topos.functors.probes.ast.entropy import calculate_kolmogorov_proxy
from topos.evaluation.policies.simple import describe_entropy_ratio
from topos.graphs.cfg.builder import _collect_callables, build_cfg_from_uast
from topos.graphs.cfg.object import ControlFlowGraph

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
    if morphism.ast and morphism.ast.uast_root:
        try:
            callables = _collect_callables(morphism.ast.uast_root)
            for c in callables:
                name = c.attributes.get("name")
                if not name:
                    # Look for an Identifier child (common in most UAST mappings)
                    for child in c.children:
                        if child.kind == "Identifier":
                            s = child.span
                            name = morphism.source[s.start_byte:s.end_byte]
                            break
                if not name:
                    name = c.attributes.get("scope") or "anonymous"

                blocks, edges, entry_id, exit_id = build_cfg_from_uast(c)
                cfg = ControlFlowGraph(blocks=blocks, edges=edges, entry_id=entry_id, exit_id=exit_id)
                all_funcs[name] = cyclomatic_complexity(cfg)
        except Exception:
            pass

    top_funcs = dict(
        sorted(all_funcs.items(), key=lambda kv: -kv[1])[: params.top_n_functions]
    )

    ratio = calculate_kolmogorov_proxy(morphism.source)
    interpretation = describe_entropy_ratio(ratio)

    return InspectionResult(
        evaluation=evaluation,
        functions=top_funcs,
        total_functions=len(all_funcs),
        entropy_compression_ratio=ratio,
        entropy_interpretation=interpretation,
    )
