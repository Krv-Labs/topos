"""
Detailed inspection of a code string — every metric exposed, function table,
entropy breakdown.
"""

from __future__ import annotations

from topos.core.morphism import ProgramMorphism
from topos.evaluation.policies.simple import describe_entropy_ratio
from topos.functors.probes.ast.entropy import calculate_kolmogorov_proxy
from topos.functors.probes.cfg.complexity import cyclomatic_complexity
from topos.graphs.cfg.builder import _collect_callables, build_cfg_from_uast
from topos.graphs.cfg.object import ControlFlowGraph

from ..evaluation import classify_code_string
from ..formatting import to_evaluation_result
from ..schemas import (
    EvaluationResult,
    FunctionEntry,
    InspectCodeInput,
    InspectionResult,
    LatticeElement,
    ResponseFormat,
    resolve_priority,
)
from ..security import read_safe_utf8_file
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
    source, source_error = _load_source(params)
    priority, priority_source = resolve_priority(params.preferences)
    if source_error or source is None:
        empty = EvaluationResult(
            is_parseable=False,
            lattice_element=LatticeElement.SLOP,
            lattice_symbol="⊥",
            lattice_description="evaluation failed",
            dimensions={},
            scores={},
            priority=priority,
            priority_source=priority_source,
            guidance="",
            coupling_available=False,
            warnings=_response_format_warnings(params.response_format),
            error=source_error or "source error",
        )
        return InspectionResult(
            evaluation=empty, total_functions=0, error=source_error or "source error"
        )

    try:
        result = classify_code_string(source, params.language, priority)
    except Exception as exc:
        empty = EvaluationResult(
            is_parseable=False,
            lattice_element=LatticeElement.SLOP,
            lattice_symbol="⊥",
            lattice_description="evaluation failed",
            dimensions={},
            scores={},
            priority=priority,
            priority_source=priority_source,
            guidance="",
            coupling_available=False,
            warnings=_response_format_warnings(params.response_format),
            error=str(exc),
        )
        return InspectionResult(evaluation=empty, total_functions=0, error=str(exc))

    evaluation = to_evaluation_result(
        result,
        coupling_available=False,
        preferences=params.preferences.to_preferences() if params.preferences else None,
        priority_source=priority_source,
        warnings=_response_format_warnings(params.response_format),
    )

    morphism = ProgramMorphism(source=source, language=params.language)
    all_funcs: list[FunctionEntry] = []
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
                            name = morphism.source[s.start_byte : s.end_byte]
                            break
                if not name:
                    name = c.attributes.get("scope") or "anonymous"

                blocks, edges, entry_id, exit_id = build_cfg_from_uast(c)
                cfg = ControlFlowGraph(
                    blocks=blocks, edges=edges, entry_id=entry_id, exit_id=exit_id
                )
                all_funcs.append(
                    FunctionEntry(
                        name=name,
                        line=max(1, c.span.start_line),
                        complexity=cyclomatic_complexity(cfg),
                    )
                )
        except Exception:
            pass

    top_entries = sorted(all_funcs, key=lambda entry: -entry.complexity)[
        : params.top_n_functions
    ]
    top_funcs = {entry.name: entry.complexity for entry in top_entries}

    ratio = calculate_kolmogorov_proxy(morphism.source)
    interpretation = describe_entropy_ratio(ratio)

    return InspectionResult(
        evaluation=evaluation,
        functions=top_funcs,
        function_entries=top_entries,
        total_functions=len(all_funcs),
        entropy_compression_ratio=ratio,
        entropy_interpretation=interpretation,
    )


def _load_source(params: InspectCodeInput) -> tuple[str | None, str | None]:
    if params.code is not None:
        return params.code, None
    if params.filepath is None:
        return None, "Provide exactly one of `code` or `filepath`."
    source, err = read_safe_utf8_file(params.filepath)
    if err:
        return None, err["error"]
    return source, None


def _response_format_warnings(response_format: ResponseFormat) -> list[str]:
    if response_format == ResponseFormat.MARKDOWN:
        return []
    return [
        "response_format is deprecated/no-op for MCP structured output; tools return "
        "structured content regardless of this value."
    ]
