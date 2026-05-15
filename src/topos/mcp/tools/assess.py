"""
Assessment tool — compare current vs. proposed code on the lattice.

This is the main tool for agent refactor loops. When ``filepath`` is provided,
the baseline is evaluated against the cached ``ModuleDependencyGraph`` and the
proposed AST is scored against that same graph (approximating coupling under
the refactor). Anti-gaming guardrail: if scores moved meaningfully while AST
edit distance is near zero, status becomes ``SUSPICIOUS_NO_STRUCTURAL_CHANGE``.
"""

from __future__ import annotations

from topos.core.morphism import ProgramMorphism
from topos.evaluation.characteristic_morphism import CharacteristicMorphism
from topos.evaluation.policies.base import Priority
from topos.functors.profunctors.ast.compare import calculate_ast_distance

from ..evaluation import (
    classify_code_string,
    classify_morphism,
    load_dep_graph,
    resolve_gitnexus_dir,
)
from ..formatting import to_evaluation_result
from ..schemas import (
    AssessImprovementInput,
    AssessmentResult,
    AssessmentStatus,
    EvaluationResult,
    LatticeElement,
)
from ..security import (
    read_safe_utf8_file,
    resolve_file_root,
    resolve_within_root,
)
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


@mcp.tool(
    name="topos_assess_improvement",
    tags={"assess", "workflow"},
    annotations=_READ_ONLY_ANN,
)
def topos_assess_improvement(params: AssessImprovementInput) -> AssessmentResult:
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
        gitnexus_dir = resolve_gitnexus_dir(params.gitnexus_dir, resolve_file_root())
        dep_graph = load_dep_graph(gitnexus_dir, str(resolved))
        current_morph = ProgramMorphism(source=current_src, language=params.language)
        current_res = classify_morphism(current_morph, Priority.SIMPLE, dep_graph)
        coupling_for_proposed = dep_graph is not None
    elif params.current_code:
        current_res = classify_code_string(
            params.current_code, params.language, Priority.SIMPLE
        )
        current_morph = ProgramMorphism(
            source=params.current_code, language=params.language
        )
        dep_graph = None
        coupling_for_proposed = False
    else:
        return _err_assessment(params, "Provide either `filepath` or `current_code`.")

    # ---- evaluate proposed ----
    proposed_morph = ProgramMorphism(
        source=params.proposed_code, language=params.language
    )
    proposed_res = classify_morphism(proposed_morph, Priority.SIMPLE, dep_graph)

    prefs = params.preferences.to_preferences() if params.preferences else None
    current_eval = to_evaluation_result(
        current_res, coupling_available=dep_graph is not None, preferences=prefs
    )
    proposed_eval = to_evaluation_result(
        proposed_res, coupling_available=coupling_for_proposed, preferences=prefs
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
            proposed_res.raw_metrics.get(m, 0.0) - current_res.raw_metrics.get(m, 0.0), 3
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

    return AssessmentResult(
        status=status,
        priority=Priority.SIMPLE,
        current=current_eval,
        proposed=proposed_eval,
        score_deltas=score_deltas,
        metric_deltas=metric_deltas,
        structural_distance=distance,
        similarity=similarity,
        coupling_available_for_proposed=coupling_for_proposed,
        suspicion_reason=suspicion,
    )


def _err_assessment(params: AssessImprovementInput, msg: str) -> AssessmentResult:
    empty = EvaluationResult(
        is_parseable=False,
        lattice_element=LatticeElement.SLOP,
        lattice_symbol="⊥",
        lattice_description="not evaluated",
        dimensions={},
        scores={},
        priority=Priority.SIMPLE,
        guidance="",
        coupling_available=False,
    )
    return AssessmentResult(
        status=AssessmentStatus.LATERAL_MOVE,
        priority=Priority.SIMPLE,
        current=empty,
        proposed=empty,
        score_deltas={},
        complexity_delta=0.0,
        structural_distance=None,
        similarity=None,
        coupling_available_for_proposed=False,
        error=msg,
    )
