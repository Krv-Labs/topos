"""
Assessment tool — compare current vs. proposed code on the lattice.

This is the main tool for agent refactor loops. When ``filepath`` is provided,
the baseline is evaluated against the cached ``ModuleDependencyGraph`` and the
proposed AST is scored against that same graph (approximating coupling under
the refactor). Anti-gaming guardrail: if scores moved meaningfully while AST
edit distance is near zero, status becomes ``SUSPICIOUS_NO_STRUCTURAL_CHANGE``.
"""

from __future__ import annotations

import difflib

from fastmcp.tools.base import ToolResult

from topos.core.morphism import ProgramMorphism
from topos.evaluation.characteristic_morphism import CharacteristicMorphism
from topos.functors.probes.cfg.complexity import cyclomatic_complexity
from topos.functors.profunctors.ast.compare import calculate_ast_distance
from topos.graphs.cfg.builder import _collect_callables, build_cfg_from_uast
from topos.graphs.cfg.object import ControlFlowGraph

from ..evaluation import (
    classify_code_string,
    classify_morphism,
    gitnexus_warnings,
    load_dep_graph,
    resolve_gitnexus_dir,
)
from ..formatting import to_evaluation_result, to_tool_result
from ..schemas import (
    AssessImprovementInput,
    AssessmentResult,
    AssessmentStatus,
    EvaluationResult,
    LatticeElement,
    resolve_priority,
)
from ..security import (
    read_safe_utf8_file,
    resolve_file_root,
    resolve_within_root,
)
from ..security_findings import security_findings
from ..server import mcp

_READ_ONLY_ANN = {
    "title": "Topos Refactor Assessment",
    "readOnlyHint": True,
    "destructiveHint": False,
    "idempotentHint": True,
    "openWorldHint": False,
}

# Near-zero edit distance threshold for gaming detection.
_STRUCTURAL_CHANGE_THRESHOLD = 0.02  # normalized distance
_MEANINGFUL_SCORE_DELTA = 3.0  # percentage points

# Cap the function-scoped regression diff so it stays a pinpoint, not a dump.
_REGRESSION_DIFF_MAX_LINES = 40


@mcp.tool(
    name="topos_assess_improvement",
    tags={"assess", "workflow"},
    annotations=_READ_ONLY_ANN,
)
def topos_assess_improvement(params: AssessImprovementInput) -> ToolResult:
    """Compare proposed code against the current baseline.

    **Preferred usage** — pass ``filepath`` (code loaded from disk + coupling
    scored against the cached ``ModuleDependencyGraph``). The proposed code is
    parsed, but coupling is an approximation: it uses the *current* dep graph
    for the target file, so inbound edges from other files reflect the
    pre-refactor state. That's fine for tight iteration loops.

    **Legacy usage** — pass ``current_code`` + ``proposed_code``. Coupling is
    NOT computed (AST-only).

    Anti-gaming: when scores move meaningfully but AST edit distance is near
    zero, status becomes ``SUSPICIOUS_NO_STRUCTURAL_CHANGE`` and
    ``suspicion_reason`` is populated.
    """
    priority, priority_source = resolve_priority(params.preferences)

    # ---- load baseline ----
    if params.filepath:
        resolved, err = resolve_within_root(params.filepath)
        if err or resolved is None:
            return _err_assessment(params, (err or {}).get("error", "path error"))
        if not resolved.is_file():
            return _err_assessment(params, f"Path is not a file: {resolved}")
        current_src, read_err = read_safe_utf8_file(resolved)
        if read_err or current_src is None:
            return _err_assessment(params, (read_err or {}).get("error", "read error"))
        project_root = resolve_file_root()
        gitnexus_dir = resolve_gitnexus_dir(params.gitnexus_dir, project_root)
        dep_graph = load_dep_graph(gitnexus_dir, str(resolved))
        current_morph = ProgramMorphism(source=current_src, language=params.language)
        current_res = classify_morphism(current_morph, priority, dep_graph)
        coupling_for_proposed = dep_graph is not None
        warnings = gitnexus_warnings(
            params.gitnexus_dir,
            project_root,
            gitnexus_dir,
            dep_graph_loaded=dep_graph is not None,
        )
    elif params.current_code:
        current_src = params.current_code
        current_res = classify_code_string(
            params.current_code, params.language, priority
        )
        current_morph = ProgramMorphism(
            source=params.current_code, language=params.language
        )
        dep_graph = None
        coupling_for_proposed = False
        warnings = [
            "COMPOSABLE not scored — current_code mode has no filepath or "
            "ModuleDependencyGraph context."
        ]
    else:
        return _err_assessment(params, "Provide either `filepath` or `current_code`.")

    proposed_src, proposed_err = _load_proposed_source(params)
    if proposed_err or proposed_src is None:
        return _err_assessment(
            params, proposed_err or "Unable to load proposed source."
        )

    # ---- evaluate proposed ----
    proposed_morph = ProgramMorphism(source=proposed_src, language=params.language)
    proposed_res = classify_morphism(proposed_morph, priority, dep_graph)

    prefs = params.preferences.to_preferences() if params.preferences else None
    current_findings = []
    proposed_findings = []
    if params.include_security_findings:
        if _secure_failed(current_res):
            current_findings = security_findings(current_morph.build_cpg())
        if _secure_failed(proposed_res):
            proposed_findings = security_findings(proposed_morph.build_cpg())
    # Warnings live on the top-level AssessmentResult only; the nested
    # current/proposed evals would otherwise duplicate the identical list.
    current_eval = to_evaluation_result(
        current_res,
        coupling_available=dep_graph is not None,
        preferences=prefs,
        priority_source=priority_source,
        security_findings=current_findings,
    )
    proposed_eval = to_evaluation_result(
        proposed_res,
        coupling_available=coupling_for_proposed,
        preferences=prefs,
        priority_source=priority_source,
        security_findings=proposed_findings,
    )

    # ---- score & metric deltas ----
    all_dims = set(current_eval.scores) | set(proposed_eval.scores)
    score_deltas = {
        dim: round(
            proposed_eval.scores.get(dim, 0.0) - current_eval.scores.get(dim, 0.0), 1
        )
        for dim in all_dims
    }

    all_metrics = set(current_res.raw_metrics) | set(proposed_res.raw_metrics)
    metric_deltas = {
        m: round(
            proposed_res.raw_metrics.get(m, 0.0) - current_res.raw_metrics.get(m, 0.0),
            3,
        )
        for m in all_metrics
    }

    # ---- structural distance ----
    distance = None
    similarity = None
    if current_res.is_parseable and proposed_res.is_parseable:
        dist = calculate_ast_distance(current_morph.ast, proposed_morph.ast)
        distance = dist.normalized_distance
        similarity = 1.0 - dist.normalized_distance

    # ---- lattice movement ----
    lattice = CharacteristicMorphism().omega
    cur_summary = current_res.summary()
    prop_summary = proposed_res.summary()
    lattice_changed = cur_summary != prop_summary
    lattice_improved = lattice_changed and lattice.leq(cur_summary, prop_summary)
    lattice_regressed = lattice_changed and lattice.leq(prop_summary, cur_summary)

    score_improved = any(d > 0 for d in score_deltas.values())
    score_regressed = any(d < 0 for d in score_deltas.values())

    # ---- status classification ----
    status = AssessmentStatus.LATERAL_MOVE
    if lattice_improved:
        status = AssessmentStatus.IMPROVEMENT
    elif lattice_regressed:
        status = AssessmentStatus.REGRESSION
    elif score_improved and not score_regressed:
        status = AssessmentStatus.IMPROVEMENT_SCORE
    elif score_regressed and not score_improved:
        status = AssessmentStatus.REGRESSION_SCORE

    # ---- anti-gaming check ----
    suspicion = None
    if (
        distance is not None
        and distance < _STRUCTURAL_CHANGE_THRESHOLD
        and status
        in (
            AssessmentStatus.IMPROVEMENT,
            AssessmentStatus.IMPROVEMENT_SCORE,
        )
        and any(abs(d) >= _MEANINGFUL_SCORE_DELTA for d in score_deltas.values())
    ):
        status = AssessmentStatus.SUSPICIOUS_NO_STRUCTURAL_CHANGE
        suspicion = (
            f"Scores improved (deltas={score_deltas}) but normalized AST edit "
            f"distance is only {distance:.3f} — the tree barely changed. Either "
            "the refactor is trivially cosmetic (comment/whitespace shuffle) "
            "or the scoring is oscillating. Re-verify with a concrete "
            "structural change."
        )

    # ---- regression pinpoint ----
    # On a regression/suspicious verdict, give the agent a function-scoped diff
    # of the single worst function instead of forcing a full metric-tree diff.
    regression_diff = None
    if status in _REGRESSION_STATUSES:
        regression_diff = _regression_diff(current_src, proposed_src, params.language)

    model = AssessmentResult(
        status=status,
        priority=priority,
        priority_source=priority_source,
        current=current_eval,
        proposed=proposed_eval,
        score_deltas=score_deltas,
        metric_deltas=metric_deltas,
        structural_distance=distance,
        similarity=similarity,
        coupling_available_for_proposed=coupling_for_proposed,
        warnings=warnings,
        suspicion_reason=suspicion,
        regression_diff=regression_diff,
    )
    return to_tool_result(model, render_assessment_md(model))


def _err_assessment(params: AssessImprovementInput, msg: str) -> ToolResult:
    priority, priority_source = resolve_priority(params.preferences)
    empty = EvaluationResult(
        is_parseable=False,
        lattice_element=LatticeElement.SLOP,
        lattice_symbol="⊥",
        lattice_description="not evaluated",
        dimensions={},
        scores={},
        priority=priority,
        priority_source=priority_source,
        guidance="",
        coupling_available=False,
    )
    model = AssessmentResult(
        status=AssessmentStatus.LATERAL_MOVE,
        priority=priority,
        priority_source=priority_source,
        current=empty,
        proposed=empty,
        score_deltas={},
        structural_distance=None,
        similarity=None,
        coupling_available_for_proposed=False,
        error=msg,
    )
    return to_tool_result(model, render_assessment_md(model))


def _load_proposed_source(
    params: AssessImprovementInput,
) -> tuple[str | None, str | None]:
    if params.proposed_code is not None:
        return params.proposed_code, None
    if params.proposed_filepath is None:
        return None, "Provide exactly one of `proposed_code` or `proposed_filepath`."
    source, err = read_safe_utf8_file(params.proposed_filepath)
    if err:
        return None, err["error"]
    return source, None


def _secure_failed(result) -> bool:
    return bool(
        result.raw_metrics.get("cpg.dangerous_calls", 0.0) > 0
        or result.raw_metrics.get("cpg.taint_flows", 0.0) > 0
    )


# ---------------------------------------------------------------------------
# Regression pinpoint — function-scoped unified diff
# ---------------------------------------------------------------------------

# Statuses that warrant a targeted regression diff.
_REGRESSION_STATUSES = frozenset(
    {
        AssessmentStatus.REGRESSION,
        AssessmentStatus.REGRESSION_SCORE,
        AssessmentStatus.SUSPICIOUS_NO_STRUCTURAL_CHANGE,
    }
)


def _function_complexities(
    source: str, language: str
) -> dict[str, tuple[int, list[str]]]:
    """Map function name -> (cyclomatic_complexity, source_lines).

    Mirrors the callable-collection pattern in ``inspect.py``. Source lines are
    sliced by the UAST byte span so they round-trip exactly into difflib.
    """
    out: dict[str, tuple[int, list[str]]] = {}
    morph = ProgramMorphism(source=source, language=language)
    if not (morph.ast and morph.ast.uast_root):
        return out
    try:
        callables = _collect_callables(morph.ast.uast_root)
    except Exception:
        return out
    for c in callables:
        name = c.attributes.get("name")
        if not name:
            for child in c.children:
                if child.kind == "Identifier":
                    s = child.span
                    name = morph.source[s.start_byte : s.end_byte]
                    break
        if not name:
            name = c.attributes.get("scope") or "anonymous"
        if name in out:
            # Overloads / duplicate names: skip rather than guess which moved.
            continue
        try:
            blocks, edges, entry_id, exit_id = build_cfg_from_uast(c)
            cfg = ControlFlowGraph(
                blocks=blocks, edges=edges, entry_id=entry_id, exit_id=exit_id
            )
            complexity = cyclomatic_complexity(cfg)
        except Exception:
            continue
        span = c.span
        body = morph.source[span.start_byte : span.end_byte]
        # No keepends: difflib + lineterm="" then a "\n".join keeps lines clean.
        out[name] = (complexity, body.splitlines())
    return out


def _regression_diff(current_src: str, proposed_src: str, language: str) -> str | None:
    """Unified diff of the single function with the worst complexity increase.

    Returns ``None`` (rather than a whole-file diff) when no function got more
    complex, or when function matching is ambiguous — keeps the output lean and
    actionable. stdlib ``difflib`` only.
    """
    cur = _function_complexities(current_src, language)
    prop = _function_complexities(proposed_src, language)
    if not cur or not prop:
        return None

    # Match by name; find the largest ADVERSE complexity increase.
    worst_name: str | None = None
    worst_delta = 0
    for name, (prop_cx, _) in prop.items():
        if name not in cur:
            # Rename/add — don't dump a whole-function diff. Fallback: None.
            continue
        delta = prop_cx - cur[name][0]
        if delta > worst_delta:
            worst_delta = delta
            worst_name = name
    if worst_name is None:
        return None

    cur_cx, cur_lines = cur[worst_name]
    prop_cx, prop_lines = prop[worst_name]
    diff_lines = list(
        difflib.unified_diff(
            cur_lines,
            prop_lines,
            fromfile=f"{worst_name} (current)",
            tofile=f"{worst_name} (proposed)",
            lineterm="",
        )
    )
    if not diff_lines:
        return None

    header = (
        f"# regression in `{worst_name}`: cyclomatic complexity "
        f"{cur_cx} -> {prop_cx} ({prop_cx - cur_cx:+d})"
    )
    body = diff_lines
    if len(body) > _REGRESSION_DIFF_MAX_LINES:
        hidden = len(body) - _REGRESSION_DIFF_MAX_LINES
        body = body[:_REGRESSION_DIFF_MAX_LINES]
        body.append(f"# ... (truncated, {hidden} more lines)")
    return "\n".join([header, *body])


# ---------------------------------------------------------------------------
# Markdown renderer (rendered into ToolResult.content)
# ---------------------------------------------------------------------------

_STATUS_MEANING: dict[AssessmentStatus, str] = {
    AssessmentStatus.IMPROVEMENT: "moved up the lattice",
    AssessmentStatus.IMPROVEMENT_SCORE: "same verdict, scores improved",
    AssessmentStatus.LATERAL_MOVE: "no verdict or score movement",
    AssessmentStatus.REGRESSION: "moved down the lattice",
    AssessmentStatus.REGRESSION_SCORE: "same verdict, scores regressed",
    AssessmentStatus.SUSPICIOUS_NO_STRUCTURAL_CHANGE: (
        "scores moved but the AST barely changed"
    ),
}


def render_assessment_md(r: AssessmentResult) -> str:
    """Compact markdown for a refactor assessment.

    Summarizes current vs. proposed rather than dumping both full evaluations;
    the structured_content channel still carries everything.
    """
    if r.error:
        return f"**Error:** {r.error}"
    meaning = _STATUS_MEANING.get(r.status, "")
    lines = [f"**Status:** {r.status.value} — {meaning}"]
    lines.append(f"**Priority:** `{r.priority.value}`")
    lines.append(
        f"**Verdict:** {r.current.lattice_element.value} → "
        f"{r.proposed.lattice_element.value}"
    )
    if r.structural_distance is not None:
        sim = f", similarity {r.similarity:.3f}" if r.similarity is not None else ""
        lines.append(f"**Structural distance:** {r.structural_distance:.3f}{sim}")
    if r.score_deltas:
        deltas = ", ".join(f"{k}={v:+.1f}" for k, v in sorted(r.score_deltas.items()))
        lines.append(f"**Score deltas:** {deltas}")
    # Only surface metrics that actually moved, to keep this lean.
    moved = {m: d for m, d in r.metric_deltas.items() if d != 0.0}
    if moved:
        md = ", ".join(f"`{m}`={d:+.3f}" for m, d in sorted(moved.items()))
        lines.append(f"**Metric deltas:** {md}")
    if r.suspicion_reason:
        lines.append(f"> ⚠️ {r.suspicion_reason}")
    if r.regression_diff:
        lines.append("")
        lines.append("## Regression diff")
        lines.append("```diff")
        lines.append(r.regression_diff)
        lines.append("```")
    return "\n".join(lines)
