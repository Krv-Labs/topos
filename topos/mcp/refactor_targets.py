"""Build ranked refactor targets from existing evaluation evidence.

Targets are derived from the same canonical sources the evaluation itself
uses: gate decisions come from :mod:`topos.evaluation.policies.gates` (so a
target can never contradict the score, including entrypoint exemptions) and
security operations from :mod:`topos.evaluation.security_guidance` (the same
suffix-matched table the suggestion engine renders as prose).
"""

from __future__ import annotations

from hashlib import sha1
from pathlib import Path

from topos.evaluation.characteristic_morphism import ClassificationResult
from topos.evaluation.policies.gates import GATES_BY_METRIC, evaluate_gates
from topos.evaluation.security_guidance import remediation_for

from .schemas import (
    FunctionEntry,
    RefactorTarget,
    SecurityFinding,
)

_LOCATION_CONSTRAINTS = ["preserve public behavior"]
_MODULE_METRIC_CONSTRAINTS = [
    "preserve module API unless the caller requested an API change"
]
_SECURITY_CONSTRAINTS = [
    "do not allowlist unless the risk is intentional and documented"
]
_DEFAULT_PILLAR_RANK = {"simple": 0, "secure": 1, "composable": 2}


def build_refactor_targets(
    *,
    filepath: str,
    result: ClassificationResult,
    security_findings: list[SecurityFinding],
    locations: dict[str, list[FunctionEntry]],
    ranking=None,
    max_targets: int = 5,
) -> list[RefactorTarget]:
    """Rank concrete edit targets without rerunning classification."""
    candidates: list[RefactorTarget] = []
    for metric, entries in locations.items():
        candidates.extend(
            _location_target(filepath, metric, entry) for entry in entries
        )
    candidates.extend(_module_metric_targets(filepath, result))
    candidates.extend(_security_targets(filepath, security_findings))

    pillar_rank = dict(_DEFAULT_PILLAR_RANK)
    if ranking:
        pillar_rank = {getattr(g, "value", str(g)): i for i, g in enumerate(ranking)}
    return sorted(candidates, key=lambda t: _rank_key(t, pillar_rank))[:max_targets]


def _location_target(
    filepath: str, metric: str, entry: FunctionEntry
) -> RefactorTarget:
    """A target for one offending function span (or whole-module marker)."""
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
        failing_generators=[GATES_BY_METRIC[metric].pillar],
        metric=metric,
        current_value=float(entry.complexity),
        threshold=GATES_BY_METRIC[metric].high,
        severity="fix",
        recommended_operations=operations,
        constraints=_LOCATION_CONSTRAINTS,
        evidence={
            "complexity": entry.complexity,
            "metric_source": entry.metric_source,
            "includes_nested": entry.includes_nested,
        },
    )


def _module_metric_targets(
    filepath: str, result: ClassificationResult
) -> list[RefactorTarget]:
    """Targets for failing module-granularity gates (entropy, coupling)."""
    return [
        RefactorTarget(
            target_id=_target_id(filepath, r.spec.metric, "<module>", 1),
            kind="module",
            filepath=filepath,
            symbol="<module>",
            line_start=1,
            failing_generators=[r.spec.pillar],
            metric=r.spec.metric,
            current_value=r.value,
            threshold=r.threshold,
            severity="fix",
            recommended_operations=list(r.operations),
            constraints=_MODULE_METRIC_CONSTRAINTS,
            evidence={"interpretation": result.interpretation.get(r.spec.metric)},
        )
        for r in evaluate_gates(
            result.raw_metrics,
            is_entrypoint_module=result.is_entrypoint_module,
        )
        if not r.passed and r.spec.granularity == "module" and r.spec.pillar != "secure"
    ]


def _security_targets(
    filepath: str, findings: list[SecurityFinding]
) -> list[RefactorTarget]:
    targets: list[RefactorTarget] = []
    for finding in findings:
        _, operations = remediation_for(finding)
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
                recommended_operations=list(operations),
                constraints=_SECURITY_CONSTRAINTS,
                evidence={
                    "kind": finding.kind,
                    "snippet": finding.snippet,
                    "source": finding.source,
                    "sink": finding.sink,
                },
            )
        )
    return targets


def _rank_key(
    target: RefactorTarget, pillar_rank: dict[str, int]
) -> tuple[int, int, int, str]:
    pillar = next(iter(target.failing_generators), "simple")
    rank = pillar_rank.get(pillar, _DEFAULT_PILLAR_RANK.get(pillar, 99))
    current = target.current_value or 0.0
    threshold = target.threshold if target.threshold is not None else current
    excess = int(abs(current - threshold) * 100)
    return (rank, -excess, target.line_start or 0, target.target_id)


def _target_id(filepath: str, metric: str, symbol: str | None, line: int | None) -> str:
    raw = f"{Path(filepath).as_posix()}:{metric}:{symbol or ''}:{line or ''}"
    return "rt_" + sha1(raw.encode("utf-8")).hexdigest()[:12]
