"""
Detailed inspection of a code string — every metric exposed, function table,
entropy breakdown.
"""

from __future__ import annotations

from pathlib import Path

from fastmcp.tools.base import ToolResult

from topos.core.morphism import ProgramMorphism
from topos.evaluation.policies.simple import describe_entropy_ratio
from topos.functors.probes.ast.entropy import calculate_kolmogorov_proxy
from topos.functors.probes.cfg.complexity import cyclomatic_complexity
from topos.graphs.cfg.builder import _collect_callables, build_cfg_from_uast
from topos.graphs.cfg.object import ControlFlowGraph

from ..diagnostics import overlay_for_source
from ..evaluation import classify_code_string
from ..formatting import render_evaluation_md, to_evaluation_result, to_tool_result
from ..schemas import (
    EvaluationResult,
    FunctionEntry,
    InspectCodeInput,
    InspectionResult,
    LatticeElement,
    resolve_priority,
)
from ..security import read_safe_utf8_file, resolve_within_root
from ..server import mcp

_READ_ONLY_ANN = {
    "title": "Topos Detailed Inspection",
    "readOnlyHint": True,
    "destructiveHint": False,
    "idempotentHint": True,
    "openWorldHint": False,
}


def _span_text(source_bytes: bytes, span) -> str:
    """Slice a UAST byte span out of the UTF-8-encoded source.

    UAST offsets are byte offsets, so we must index the encoded bytes, not the
    code-point-indexed str. Bounds-guarded like ``cpg/object.py`` in case the
    span refers to a different revision than ``source_bytes``.
    """
    if span.end_byte > len(source_bytes):
        return ""
    return source_bytes[span.start_byte : span.end_byte].decode(
        "utf-8", errors="replace"
    )


@mcp.tool(
    name="topos_inspect_code",
    tags={"inspect"},
    annotations=_READ_ONLY_ANN,
)
def topos_inspect_code(params: InspectCodeInput) -> ToolResult:
    """Full metric breakdown for a code string.

    Returns the lattice evaluation, a *top-N* function complexity table
    (sorted descending; configurable via ``top_n_functions``), and entropy
    details. The top-N cap prevents large files from dumping hundreds of
    functions and blowing out agent context.
    """
    source, source_error, file_path = _load_source(params)
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
            error=source_error or "source error",
        )
        model = InspectionResult(
            evaluation=empty, total_functions=0, error=source_error or "source error"
        )
        return to_tool_result(model, render_inspection_md(model))

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
            error=str(exc),
        )
        model = InspectionResult(evaluation=empty, total_functions=0, error=str(exc))
        return to_tool_result(model, render_inspection_md(model))

    evaluation = to_evaluation_result(
        result,
        coupling_available=False,
        preferences=params.preferences.to_preferences() if params.preferences else None,
        priority_source=priority_source,
        **_overlay_kwargs(
            overlay_for_source(
                source,
                params.language,
                result,
                file_path=file_path,
                allows=params.allow,
                include_security_findings=True,
            )
        ),
        verbose=params.verbose,
    )

    morphism = ProgramMorphism(source=source, language=params.language)
    # UAST spans are UTF-8 byte offsets; encode ONCE before the loop and slice
    # the bytes (not the code-point-indexed str) so non-ASCII chars don't shift
    # the extracted identifiers. Same idiom as complexity.py / cpg/object.py.
    source_bytes = morphism.source.encode("utf-8")
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
                            name = _span_text(source_bytes, child.span)
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

    model = InspectionResult(
        evaluation=evaluation,
        functions=top_funcs,
        function_entries=top_entries,
        total_functions=len(all_funcs),
        entropy_compression_ratio=ratio,
        entropy_interpretation=interpretation,
    )
    return to_tool_result(model, render_inspection_md(model, verbose=params.verbose))


def _load_source(
    params: InspectCodeInput,
) -> tuple[str | None, str | None, Path | None]:
    if params.code is not None:
        return params.code, None, None
    if params.filepath is None:
        return None, "Provide exactly one of `code` or `filepath`.", None
    resolved, resolve_err = resolve_within_root(params.filepath)
    if resolve_err or resolved is None:
        return None, (resolve_err or {}).get("error", "path error"), None
    source, err = read_safe_utf8_file(params.filepath)
    if err:
        return None, err["error"], None
    return source, None, resolved


def _overlay_kwargs(overlay):
    if overlay is None:
        return {}
    return {
        "security_findings": overlay.active_findings,
        "acknowledged_risks": overlay.acknowledged_risks,
        "adjusted_verdict": overlay.verdict,
    }


# ---------------------------------------------------------------------------
# Markdown renderer (rendered into ToolResult.content)
# ---------------------------------------------------------------------------


def render_inspection_md(r: InspectionResult, *, verbose: bool = True) -> str:
    """Compact markdown for a full inspection breakdown."""
    if r.error:
        return f"**Error:** {r.error}"
    e = r.evaluation
    lines = [f"**Lattice:** {e.lattice_symbol} {e.lattice_element.value}"]
    lines.append(f"**Total functions:** {r.total_functions}")
    if r.function_entries:
        lines.append("")
        lines.append("## Top functions (by complexity)")
        lines.append("| Function | Line | Complexity |")
        lines.append("| --- | ---: | ---: |")
        for fn in r.function_entries:
            # Sanitize the cell: a stray newline or `|` in a name would break
            # the markdown table layout — collapse them so rendering is robust.
            safe_name = (
                fn.name.replace("\n", " ").replace("\r", " ").replace("|", "\\|")
            )
            lines.append(f"| `{safe_name}` | {fn.line} | {fn.complexity} |")
    if r.entropy_compression_ratio is not None:
        lines.append("")
        interp = f" — {r.entropy_interpretation}" if r.entropy_interpretation else ""
        lines.append(
            f"**Entropy compression ratio:** {r.entropy_compression_ratio:.3f}{interp}"
        )
    # Embed the full evaluation block (reuses the shared renderer).
    lines.append("")
    lines.append(render_evaluation_md(e, title="Evaluation", verbose=verbose))
    return "\n".join(lines)
