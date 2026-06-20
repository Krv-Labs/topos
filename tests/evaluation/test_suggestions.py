from __future__ import annotations

from topos.core.omega import EvaluationValue
from topos.evaluation.characteristic_morphism import ClassificationResult
from topos.evaluation.policies import Priority
from topos.evaluation.suggestions import suggest_refactors
from topos.mcp.schemas import SecurityFinding


def _result(
    *,
    dimensions: dict[str, EvaluationValue],
    raw_metrics: dict[str, float],
    element: EvaluationValue,
) -> ClassificationResult:
    return ClassificationResult(
        is_parseable=True,
        dimensions=dimensions,
        scores={d: 0.0 for d in dimensions},
        lattice_element=element,
        priority=Priority.SECURE,
        raw_metrics=raw_metrics,
        interpretation={},
    )


def test_eval_finding_yields_secure_fix_naming_callee() -> None:
    result = _result(
        dimensions={"secure": EvaluationValue.SLOP},
        raw_metrics={"cpg.dangerous_calls": 1.0, "cpg.taint_flows": 0.0},
        element=EvaluationValue.SLOP,
    )
    finding = SecurityFinding(
        kind="dangerous_call", line=2, snippet="return eval(x)", callee="eval"
    )

    suggestions = suggest_refactors(result, active_findings=[finding])

    secure = [s for s in suggestions if s.pillar == "secure"]
    assert secure, "expected a SECURE suggestion for an eval finding"
    assert secure[0].severity == "fix"
    assert "eval" in secure[0].message


def test_high_cyclomatic_yields_simple_suggestion() -> None:
    result = _result(
        dimensions={"simple": EvaluationValue.SLOP},
        raw_metrics={"cfg.cyclomatic": 25.0, "ast.entropy": 0.5},
        element=EvaluationValue.SLOP,
    )

    suggestions = suggest_refactors(result)

    simple = [s for s in suggestions if s.metric == "cfg.cyclomatic"]
    assert simple and simple[0].severity == "fix"
    assert "cyclomatic" in simple[0].message.lower()


def test_high_fan_out_yields_composable_suggestion() -> None:
    result = _result(
        dimensions={"composable": EvaluationValue.SLOP},
        raw_metrics={"mdg.fan_out": 30.0, "mdg.instability": 0.5},
        element=EvaluationValue.SLOP,
    )

    suggestions = suggest_refactors(result)

    assert [s for s in suggestions if s.metric == "mdg.fan_out"]


def test_clean_file_yields_no_suggestions() -> None:
    result = _result(
        dimensions={
            "simple": EvaluationValue.SIMPLE,
            "secure": EvaluationValue.SECURE,
        },
        raw_metrics={
            "cfg.cyclomatic": 2.0,
            "ast.entropy": 0.5,
            "cpg.dangerous_calls": 0.0,
            "cpg.taint_flows": 0.0,
        },
        element=EvaluationValue.IDEAL,
    )

    assert suggest_refactors(result, active_findings=[]) == []


def test_allowlisted_finding_produces_no_secure_suggestion() -> None:
    # The CLI passes only NON-allowlisted findings as active_findings.
    result = _result(
        dimensions={"secure": EvaluationValue.SLOP},
        raw_metrics={"cpg.dangerous_calls": 1.0, "cpg.taint_flows": 0.0},
        element=EvaluationValue.SECURE,
    )

    suggestions = suggest_refactors(result, active_findings=[])

    assert not [s for s in suggestions if s.pillar == "secure"]
