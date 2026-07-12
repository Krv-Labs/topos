"""
Refactor-suggestion engine — turns a score into actionable next steps.

Maps the metrics that *failed* their policy gate (and any active security
findings) into concrete, imperative, refactor-focused instructions an agent
or developer can act on directly.  Gate decisions come from
:mod:`topos.evaluation.policies.gates` — the same specs the scorers consult —
so a suggestion can never fire on a gate the scorer passed (including the
entrypoint-module exemptions). Security prose comes from
:mod:`topos.evaluation.security_guidance`.

Pure and side-effect-free so both the CLI and the MCP layer can render the
same suggestions.
"""

from __future__ import annotations

from dataclasses import dataclass

from topos.evaluation.characteristic_morphism import ClassificationResult
from topos.evaluation.policies.gates import GateOutcome, GateResult, evaluate_gates
from topos.evaluation.security_guidance import remediation_for
from topos.mcp.schemas import SecurityFinding


@dataclass(frozen=True)
class Suggestion:
    """One actionable, refactor-focused next step."""

    pillar: str  # "simple" | "composable" | "secure" | "coverage"
    metric: str | None  # raw-metric key, or None for finding/guidance-derived
    severity: str  # "fix" (gate failed) | "improve" (advisory)
    message: str  # imperative instruction


def suggest_refactors(
    result: ClassificationResult,
    *,
    active_findings: list[SecurityFinding] | None = None,
) -> list[Suggestion]:
    """Build actionable suggestions from a classification result.

    *active_findings* are the security findings that are NOT allowlisted;
    only these produce SECURE suggestions.
    """
    if not result.is_parseable:
        return [
            Suggestion(
                pillar="simple",
                metric=None,
                severity="fix",
                message="Fix the parse error so the file can be evaluated.",
            )
        ]

    failing = {
        r.spec.metric: r
        for r in evaluate_gates(
            result.raw_metrics,
            is_entrypoint_module=result.is_entrypoint_module,
        )
        if not r.passed and r.spec.pillar != "secure"
    }
    suggestions = [
        Suggestion(
            pillar=failing[metric].spec.pillar,
            metric=metric,
            severity="fix",
            message=_gate_message(failing[metric]),
        )
        for metric in _SUGGESTION_ORDER
        if metric in failing
    ]

    for finding in active_findings or []:
        suggestions.append(
            Suggestion(
                pillar="secure",
                metric=finding.callee,
                severity="fix",
                message=remediation_for(finding)[0],
            )
        )
    return suggestions


# Legacy emission order (SIMPLE gates before COMPOSABLE, cyclomatic first).
_SUGGESTION_ORDER = (
    "cfg.cyclomatic",
    "ast.max_function_complexity",
    "ast.entropy",
    "mdg.instability",
    "mdg.fan_out",
    "mdg.fan_in",
)


def _gate_message(r: GateResult) -> str:
    """Imperative prose for a failed gate, quoting the real bounds."""
    value, threshold = r.value, r.threshold
    if r.spec.metric == "cfg.cyclomatic":
        return (
            f"Extract helper functions to cut branching "
            f"(cyclomatic {value:.0f} > {threshold:.0f})."
        )
    if r.spec.metric == "ast.max_function_complexity":
        return (
            f"Split the most complex function "
            f"(complexity {value:.0f} > {threshold:.0f})."
        )
    if r.spec.metric == "ast.entropy":
        if r.outcome is GateOutcome.FAIL_LOW:
            return (
                f"Consolidate repetitive/boilerplate code "
                f"(entropy {value:.2f} < {threshold})."
            )
        return (
            f"Decompose dense logic into named steps "
            f"(entropy {value:.2f} > {threshold})."
        )
    if r.spec.metric == "mdg.instability":
        return (
            f"Rebalance dependencies (instability {value:.2f}; "
            f"aim for {r.spec.low}–{r.spec.high})."
        )
    if r.spec.metric == "mdg.fan_out":
        return (
            f"Reduce fan-out {value:.0f} (> {threshold:.0f}) — "
            "introduce an interface or invert the dependency."
        )
    # mdg.fan_in
    return (
        f"Split this module (fan-in {value:.0f} > {threshold:.0f}); "
        "too many modules depend on it."
    )
