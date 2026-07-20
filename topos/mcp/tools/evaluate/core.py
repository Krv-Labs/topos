"""
Evaluation tools: code string and single file.
"""

from __future__ import annotations

from fastmcp.tools.base import ToolResult

from topos.evaluation.policies.base import Priority

from ...diagnostics import overlay_for_file, overlay_for_source
from ...evaluation import (
    classify_code_string,
    classify_file,
    detect_language,
    gitnexus_warnings,
    resolve_gitnexus_dir,
)
from ...formatting import (
    render_evaluation_md,
    to_evaluation_result,
    to_tool_result,
)
from ...metric_locations import build_metric_locations
from ...refactor_targets import build_refactor_targets
from ...schemas import (
    EvaluateCodeInput,
    EvaluateFileInput,
    EvaluationResult,
    LatticeElement,
    resolve_priority,
)
from ...security import read_safe_utf8_file, resolve_file_root, resolve_within_root
from ...server import mcp
from .render import _error_md

_READ_ONLY_ANN = {
    "title": "Topos Code Evaluation",
    "readOnlyHint": True,
    "destructiveHint": False,
    "idempotentHint": True,
    "openWorldHint": False,
}


def _overlay_kwargs(overlay):
    if overlay is None:
        return {}
    return {
        "security_findings": overlay.active_findings,
        "acknowledged_risks": overlay.acknowledged_risks,
        "adjusted_verdict": overlay.verdict,
    }


@mcp.tool(
    name="topos_evaluate_code",
    tags={"evaluate", "single-unit"},
    annotations=_READ_ONLY_ANN,
)
def topos_evaluate_code(params: EvaluateCodeInput) -> ToolResult:
    """Score a raw code string on the SIMPLE / COMPOSABLE / SECURE quality
    lattice (read-only; never writes or runs the code).

    Use for a snippet not yet on disk. Only SIMPLE and SECURE are reachable
    here (scored from the source's CFG/CPG); COMPOSABLE needs a module
    dependency graph, so for it use ``topos_evaluate_file`` with
    ``gitnexus_dir``, or ``topos_evaluate_project`` for a whole tree. Returns an
    EvaluationResult: the lattice verdict (SLOP…IDEAL), per-generator scores,
    and a next-step agent contract.
    """
    try:
        priority, priority_source = resolve_priority(params.preferences)
        result = classify_code_string(params.code, params.language, priority)
    except Exception as exc:
        model = EvaluationResult(
            is_parseable=False,
            lattice_element=LatticeElement.SLOP,
            lattice_symbol="⊥",
            lattice_description="Evaluation failed",
            dimensions={},
            scores={},
            priority=Priority.SIMPLE,
            guidance="",
            coupling_available=False,
            error=str(exc),
        )
        return to_tool_result(model, _error_md(model))
    prefs = params.preferences.to_preferences() if params.preferences else None
    model = to_evaluation_result(
        result,
        coupling_available=False,
        preferences=prefs,
        priority_source=priority_source,
        **_overlay_kwargs(
            overlay_for_source(
                params.code,
                params.language,
                result,
                allows=params.allow,
            )
        ),
        verbose=params.verbose,
        metric_locations=build_metric_locations(params.code, params.language, result),
    )
    return to_tool_result(model, render_evaluation_md(model, verbose=params.verbose))


@mcp.tool(
    name="topos_evaluate_file",
    tags={"evaluate", "single-unit"},
    annotations=_READ_ONLY_ANN,
)
def topos_evaluate_file(params: EvaluateFileInput) -> ToolResult:
    """Score a file on disk on the SIMPLE / COMPOSABLE / SECURE lattice — the
    only evaluate tool that can reach COMPOSABLE (read-only).

    Use for one file on disk. A ModuleDependencyGraph is attached when
    ``gitnexus_dir`` is given or auto-detected at ``<root>/.gitnexus``, enabling
    COMPOSABLE (SIMPLE/SECURE always run); generate that graph first with
    ``topos_generate_depgraph`` (needs ``pnpm add -g gitnexus`` or
    ``npm install -g gitnexus``). For an
    in-memory snippet use ``topos_evaluate_code``; for a whole directory use
    ``topos_evaluate_project``. ``coupling_available`` is false when no graph is
    found, leaving any COMPOSABLE verdict unreachable. Returns an
    EvaluationResult.
    """
    resolved, err = resolve_within_root(params.filepath)
    if err or resolved is None:
        model = EvaluationResult(
            is_parseable=False,
            lattice_element=LatticeElement.SLOP,
            lattice_symbol="⊥",
            lattice_description="Access denied / path error",
            dimensions={},
            scores={},
            priority=Priority.SIMPLE,
            guidance="",
            coupling_available=False,
            error=(err or {}).get("error", "path error"),
        )
        return to_tool_result(model, _error_md(model))

    if not resolved.is_file():
        model = EvaluationResult(
            is_parseable=False,
            lattice_element=LatticeElement.SLOP,
            lattice_symbol="⊥",
            lattice_description="Not a file",
            dimensions={},
            scores={},
            priority=Priority.SIMPLE,
            guidance="",
            coupling_available=False,
            error=f"Path is not a file: {resolved}",
        )
        return to_tool_result(model, _error_md(model))

    project_root = resolve_file_root()
    gitnexus_dir = resolve_gitnexus_dir(params.gitnexus_dir, project_root)
    priority, priority_source = resolve_priority(params.preferences)

    try:
        result, dep_graph = classify_file(resolved, priority, gitnexus_dir)
    except Exception as exc:
        model = EvaluationResult(
            is_parseable=False,
            lattice_element=LatticeElement.SLOP,
            lattice_symbol="⊥",
            lattice_description="Evaluation failed",
            dimensions={},
            scores={},
            priority=Priority.SIMPLE,
            guidance="",
            coupling_available=False,
            error=str(exc),
        )
        return to_tool_result(model, _error_md(model))

    prefs = params.preferences.to_preferences() if params.preferences else None
    warnings = gitnexus_warnings(
        params.gitnexus_dir,
        project_root,
        gitnexus_dir,
        dep_graph_loaded=dep_graph is not None,
    )
    overlay = overlay_for_file(resolved, result, allows=params.allow)
    source, _ = read_safe_utf8_file(resolved)
    locations = (
        build_metric_locations(source, detect_language(resolved), result)
        if source is not None
        else {}
    )
    # Targets are computed before the result model so build_agent_contract can
    # route them natively — the contract is never mutated after construction.
    targets = None
    if params.refactor_targets:
        targets = build_refactor_targets(
            filepath=str(resolved),
            result=result,
            security_findings=overlay.active_findings if overlay else [],
            locations=locations,
            ranking=params.preferences.ranking if params.preferences else None,
            max_targets=params.refactor_targets,
        )
    model = to_evaluation_result(
        result,
        coupling_available=dep_graph is not None,
        preferences=prefs,
        priority_source=priority_source,
        warnings=warnings,
        **_overlay_kwargs(overlay),
        verbose=params.verbose,
        metric_locations=locations,
        refactor_targets=targets,
        offer_refactor_targets=targets is None,
        include_security_findings=params.include_security_findings,
    )
    return to_tool_result(model, render_evaluation_md(model, verbose=params.verbose))
