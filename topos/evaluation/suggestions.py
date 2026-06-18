"""
Refactor-suggestion engine — turns a score into actionable next steps.

Maps the metrics that *failed* their policy gate (and any active security
findings) into concrete, imperative, refactor-focused instructions an agent
or developer can act on directly.  Thresholds come from the central
calibration singletons so messages quote the real numbers.

Pure and side-effect-free so both the CLI and (later) the MCP layer can
render the same suggestions.
"""

from __future__ import annotations

from dataclasses import dataclass

from topos.evaluation.characteristic_morphism import ClassificationResult
from topos.evaluation.policies.calibration import COMPOSABLE, SIMPLE
from topos.mcp.schemas import SecurityFinding


@dataclass(frozen=True)
class Suggestion:
    """One actionable, refactor-focused next step."""

    pillar: str  # "simple" | "composable" | "secure" | "coverage"
    metric: str | None  # raw-metric key, or None for finding/guidance-derived
    severity: str  # "fix" (gate failed) | "improve" (advisory)
    message: str  # imperative instruction


# Remediation phrasing keyed by dangerous callee (suffix-matched).
_REMEDIATION: dict[str, str] = {
    "eval": "Replace `eval` with `ast.literal_eval` or explicit parsing.",
    "exec": "Remove `exec`; call the code path directly or dispatch via a map.",
    "compile": "Avoid dynamic `compile`; use a static, reviewed code path.",
    "pickle.loads": (
        "Use `json` or a schema-validated deserializer instead of `pickle`."
    ),
    "marshal.loads": "Avoid `marshal`; deserialize with `json` or a safe format.",
    "yaml.load": "Use `yaml.safe_load` instead of `yaml.load`.",
    "os.system": "Replace `os.system` with `subprocess.run([...])` (no shell).",
    "os.popen": "Replace `os.popen` with `subprocess.run([...], capture_output=True)`.",
    "subprocess.call": "Pass an argument list and avoid `shell=True`.",
    "subprocess.run": "Pass an argument list and avoid `shell=True`.",
    "subprocess.popen": "Pass an argument list and avoid `shell=True`.",
    "__import__": "Import statically; avoid `__import__` on dynamic names.",
    "innerhtml": "Set text via `textContent`, or sanitize before assigning HTML.",
    "document.write": "Build DOM nodes instead of `document.write`.",
    "function": "Avoid the `Function` constructor; call a known function directly.",
    "child_process.exec": "Use `execFile`/`spawn` with an argument array (no shell).",
    "system": "Replace `system()` with an `exec*`-family call (no shell).",
    "strcpy": "Use a bounded copy (`strncpy`/`snprintf`).",
    "strcat": "Use a bounded concat (`strncat`/`snprintf`).",
    "sprintf": "Use `snprintf` with an explicit buffer size.",
    "gets": "Replace `gets` with `fgets` and an explicit length.",
    "transmute": "Avoid `mem::transmute`; use a safe conversion or `bytemuck`.",
    "unsafe": (
        "Confine or remove the `unsafe` block; document the invariant it upholds."
    ),
}


def _remediation(finding: SecurityFinding) -> str:
    if finding.kind == "taint_flow":
        src = finding.source or "untrusted input"
        sink = finding.callee or finding.sink or "the dangerous call"
        return (
            f"Validate/sanitize `{src}` before it reaches `{sink}` "
            f"(line {finding.line})."
        )
    callee = (finding.callee or "").lower()
    for key, advice in _REMEDIATION.items():
        if callee == key or callee.endswith("." + key) or callee.endswith(key):
            return advice
    return (
        f"Remove or sandbox the dangerous call `{finding.callee}` "
        f"(line {finding.line})."
    )


def suggest_refactors(
    result: ClassificationResult,
    *,
    active_findings: list[SecurityFinding] | None = None,
) -> list[Suggestion]:
    """Build actionable suggestions from a classification result.

    *active_findings* are the security findings that are NOT allowlisted;
    only these produce SECURE suggestions.
    """
    suggestions: list[Suggestion] = []
    if not result.is_parseable:
        return [
            Suggestion(
                pillar="simple",
                metric=None,
                severity="fix",
                message="Fix the parse error so the file can be evaluated.",
            )
        ]

    metrics = result.raw_metrics
    suggestions.extend(_simple_suggestions(metrics))
    suggestions.extend(_composable_suggestions(metrics))

    for finding in active_findings or []:
        suggestions.append(
            Suggestion(
                pillar="secure",
                metric=finding.callee,
                severity="fix",
                message=_remediation(finding),
            )
        )
    return suggestions


def _simple_suggestions(metrics: dict[str, float]) -> list[Suggestion]:
    out: list[Suggestion] = []
    cyclomatic = metrics.get("cfg.cyclomatic")
    if cyclomatic is not None and cyclomatic > SIMPLE.max_cyclomatic:
        out.append(
            Suggestion(
                "simple",
                "cfg.cyclomatic",
                "fix",
                f"Extract helper functions to cut branching "
                f"(cyclomatic {cyclomatic:.0f} > {SIMPLE.max_cyclomatic:.0f}).",
            )
        )
    func = metrics.get("ast.max_function_complexity")
    if func is not None and func > SIMPLE.max_function_complexity:
        out.append(
            Suggestion(
                "simple",
                "ast.max_function_complexity",
                "fix",
                f"Split the most complex function "
                f"(complexity {func:.0f} > {SIMPLE.max_function_complexity:.0f}).",
            )
        )
    entropy = metrics.get("ast.entropy")
    if entropy is not None and entropy < SIMPLE.min_entropy:
        out.append(
            Suggestion(
                "simple",
                "ast.entropy",
                "fix",
                f"Consolidate repetitive/boilerplate code "
                f"(entropy {entropy:.2f} < {SIMPLE.min_entropy}).",
            )
        )
    elif entropy is not None and entropy > SIMPLE.max_entropy:
        out.append(
            Suggestion(
                "simple",
                "ast.entropy",
                "fix",
                f"Decompose dense logic into named steps "
                f"(entropy {entropy:.2f} > {SIMPLE.max_entropy}).",
            )
        )
    return out


def _composable_suggestions(metrics: dict[str, float]) -> list[Suggestion]:
    out: list[Suggestion] = []
    instability = metrics.get("mdg.instability")
    if instability is not None and not (
        COMPOSABLE.instability_low <= instability <= COMPOSABLE.instability_high
    ):
        out.append(
            Suggestion(
                "composable",
                "mdg.instability",
                "fix",
                f"Rebalance dependencies (instability {instability:.2f}; "
                f"aim for {COMPOSABLE.instability_low}–{COMPOSABLE.instability_high}).",
            )
        )
    fan_out = metrics.get("mdg.fan_out")
    if fan_out is not None and fan_out > COMPOSABLE.max_fan_out:
        out.append(
            Suggestion(
                "composable",
                "mdg.fan_out",
                "fix",
                f"Reduce fan-out {fan_out:.0f} (> {COMPOSABLE.max_fan_out:.0f}) — "
                "introduce an interface or invert the dependency.",
            )
        )
    fan_in = metrics.get("mdg.fan_in")
    if fan_in is not None and fan_in > COMPOSABLE.max_fan_in:
        out.append(
            Suggestion(
                "composable",
                "mdg.fan_in",
                "fix",
                (
                    f"Split this module (fan-in {fan_in:.0f} > "
                    f"{COMPOSABLE.max_fan_in:.0f}); "
                    "too many modules depend on it."
                ),
            )
        )
    return out
