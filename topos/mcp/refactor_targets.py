"""Build ranked refactor targets from existing evaluation evidence."""

from __future__ import annotations

from hashlib import sha1
from pathlib import Path

from topos.evaluation.policies.calibration import SIMPLE

from .refactor_metric_specs import module_metric_specs
from .schemas import (
    EvaluationResult,
    FunctionEntry,
    RefactorTarget,
    SecurityFinding,
)

VERIFY_REFACTOR_TARGET = [
    "topos_assess_worktree_change",
    "behavior tests or type/lint checks when available",
]

_SECURITY_KIND_OPERATIONS = {
    "taint_flow": ["validate_input", "sanitize_before_sink"],
}
_SECURITY_CALLEE_OPERATIONS = {
    "eval": ["replace_dynamic_execution", "use_static_dispatch"],
    "exec": ["replace_dynamic_execution", "use_static_dispatch"],
    "compile": ["replace_dynamic_execution", "use_static_dispatch"],
    "__import__": ["replace_dynamic_execution", "use_static_dispatch"],
    "os.system": ["remove_shell_execution", "pass_argument_array"],
    "os.popen": ["remove_shell_execution", "pass_argument_array"],
    "subprocess.call": ["remove_shell_execution", "pass_argument_array"],
    "subprocess.run": ["remove_shell_execution", "pass_argument_array"],
    "subprocess.popen": ["remove_shell_execution", "pass_argument_array"],
}
_SECURITY_DEFAULT_OPERATIONS = ["replace_dangerous_api", "validate_input"]
_PILLAR_BY_PREFIX = {"mdg": "composable", "cpg": "secure"}


def build_refactor_targets(
    *,
    filepath: str,
    evaluation: EvaluationResult,
    raw_metrics: dict[str, float],
    interpretation: dict[str, str],
    locations: dict[str, list[FunctionEntry]],
    ranking=None,
    max_targets: int = 5,
    include_module_targets: bool = True,
) -> list[RefactorTarget]:
    """Rank concrete edit targets without rerunning classification."""
    candidates: list[RefactorTarget] = []
    for metric, entries in locations.items():
        for entry in entries:
            if entry.kind == "module" and not include_module_targets:
                continue
            candidates.append(_simple_target(filepath, metric, entry, evaluation))
    if include_module_targets:
        candidates.extend(_metric_targets(filepath, raw_metrics, interpretation))
    candidates.extend(_security_targets(filepath, evaluation.security_findings))
    return sorted(candidates, key=lambda t: _rank_key(t, ranking))[:max_targets]


def _simple_target(
    filepath: str, metric: str, entry: FunctionEntry, evaluation: EvaluationResult
) -> RefactorTarget:
    threshold = (
        SIMPLE.max_function_complexity
        if metric == "ast.max_function_complexity"
        else SIMPLE.max_cyclomatic
    )
    operations = (
        ["extract_helper", "split_decision_logic"]
        if entry.kind != "module"
        else ["split_module", "extract_cohesive_unit"]
    )
    return RefactorTarget(
        target_id=_target_id(
            filepath, metric, entry.qualified_name or entry.name, entry.line
        ),
        kind="module" if entry.kind == "module" else "function",
        filepath=filepath,
        symbol=entry.qualified_name or entry.name,
        line_start=entry.start_line or entry.line,
        line_end=entry.end_line,
        failing_generators=["simple"],
        metric=metric,
        current_value=float(entry.complexity),
        threshold=float(threshold),
        severity="fix",
        recommended_operations=operations,
        constraints=[
            "preserve public behavior",
            "make one focused structural change before assessing",
        ],
        verify_with=VERIFY_REFACTOR_TARGET,
        evidence={
            "complexity": entry.complexity,
            "metric_source": entry.metric_source,
            "includes_nested": entry.includes_nested,
            "guidance": evaluation.guidance,
        },
    )


def _metric_targets(
    filepath: str, raw_metrics: dict[str, float], interpretation: dict[str, str]
) -> list[RefactorTarget]:
    targets: list[RefactorTarget] = []
    for metric, current, threshold, operations in module_metric_specs(raw_metrics):
        targets.append(
            RefactorTarget(
                target_id=_target_id(filepath, metric, "<module>", 1),
                kind="module",
                filepath=filepath,
                symbol="<module>",
                line_start=1,
                failing_generators=[_pillar_for_metric(metric)],
                metric=metric,
                current_value=current,
                threshold=threshold,
                severity="fix",
                recommended_operations=operations,
                constraints=[
                    "preserve module API unless the caller requested an API change",
                    "verify with Topos assessment after editing",
                ],
                verify_with=VERIFY_REFACTOR_TARGET,
                evidence={"interpretation": interpretation.get(metric)},
            )
        )
    return targets


def _security_targets(
    filepath: str, findings: list[SecurityFinding]
) -> list[RefactorTarget]:
    targets: list[RefactorTarget] = []
    for finding in findings:
        targets.append(
            RefactorTarget(
                target_id=_target_id(
                    filepath,
                    finding.kind,
                    finding.callee or finding.snippet,
                    finding.line,
                ),
                kind="security_call",
                filepath=filepath,
                symbol=finding.callee,
                line_start=finding.line,
                line_end=finding.line,
                failing_generators=["secure"],
                metric=finding.callee or finding.kind,
                current_value=1.0,
                threshold=0.0,
                severity="fix",
                recommended_operations=_security_operations(finding),
                constraints=[
                    "do not allowlist unless the risk is intentional and documented",
                    "prefer static dispatch or argument arrays over dynamic execution",
                ],
                verify_with=VERIFY_REFACTOR_TARGET,
                evidence={
                    "kind": finding.kind,
                    "snippet": finding.snippet,
                    "source": finding.source,
                    "sink": finding.sink,
                },
            )
        )
    return targets


def _security_operations(finding: SecurityFinding) -> list[str]:
    callee = (finding.callee or "").lower()
    return (
        _SECURITY_KIND_OPERATIONS.get(finding.kind)
        or _SECURITY_CALLEE_OPERATIONS.get(callee)
        or _SECURITY_DEFAULT_OPERATIONS
    )


def _pillar_for_metric(metric: str) -> str:
    return _PILLAR_BY_PREFIX.get(metric.split(".", 1)[0], "simple")


def _rank_key(target: RefactorTarget, ranking) -> tuple[int, int, int, str]:
    pillar_rank = {getattr(g, "value", str(g)): i for i, g in enumerate(ranking or [])}
    default_rank = {"simple": 0, "secure": 1, "composable": 2}
    pillar = next(iter(target.failing_generators), "simple")
    rank = pillar_rank.get(pillar, default_rank.get(pillar, 99))
    current = target.current_value or 0.0
    threshold = target.threshold or current
    excess = int(abs(current - threshold) * 100)
    return (rank, -excess, target.line_start or 0, target.target_id)


def _target_id(filepath: str, metric: str, symbol: str | None, line: int | None) -> str:
    raw = f"{Path(filepath).as_posix()}:{metric}:{symbol or ''}:{line or ''}"
    return "rt_" + sha1(raw.encode("utf-8")).hexdigest()[:12]
