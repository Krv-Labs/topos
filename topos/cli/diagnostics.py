"""
Shared CLI renderers for actionable diagnostics (issues #58 + #63).

Bridges the MCP-only security findings, the allowlist overlay, and the
refactor-suggestion engine into the ``inspect`` / ``evaluate`` commands.
Building the CPG is lazy (only when SECURE actually has something to report)
and degrades gracefully — a CPG build failure skips the sections, it never
crashes the command.
"""

from __future__ import annotations

from pathlib import Path

import click

from topos.config import ToposConfig
from topos.core.omega import EvaluationValue
from topos.evaluation.characteristic_morphism import ClassificationResult
from topos.evaluation.suggestions import Suggestion, suggest_refactors
from topos.evaluation.suppression import AdjustedVerdict, apply_allowlist
from topos.mcp.schemas import SecurityFinding

_RULE = "-" * 40
_KIND_LABEL = {"dangerous_call": "Dangerous Call", "taint_flow": "Taint Flow"}


def collect_findings_and_verdict(
    path: str | Path,
    result: ClassificationResult,
    config: ToposConfig,
) -> tuple[list[SecurityFinding], list[tuple[SecurityFinding, object]], AdjustedVerdict]:
    """Build security findings (lazily) and the raw/adjusted verdict for *path*."""
    dangerous = result.raw_metrics.get("cpg.dangerous_calls", 0.0)
    taint = result.raw_metrics.get("cpg.taint_flows", 0.0)

    findings: list[SecurityFinding] = []
    cpg = None
    if (dangerous > 0 or taint > 0) and result.is_parseable:
        cpg = _build_cpg(path)
        if cpg is not None:
            from topos.mcp.security_findings import security_findings

            findings = security_findings(cpg)

    verdict = apply_allowlist(
        result, findings, config, file_path=str(path), cpg=cpg
    )
    return verdict.active_findings, verdict.acknowledged, verdict


def _build_cpg(path: str | Path):  # type: ignore[no-untyped-def]
    try:
        from topos.core.morphism import ProgramMorphism

        return ProgramMorphism.from_file(str(path)).build_cpg()
    except (OSError, ValueError):
        return None


def render_security_findings(
    active: list[SecurityFinding],
    acknowledged: list[tuple[SecurityFinding, object]],
    *,
    indent: str = "",
) -> None:
    """Print the Security Findings section (issue #58 mockup)."""
    if not active and not acknowledged:
        return
    click.echo()
    click.echo(f"{indent}Security Findings")
    click.echo(f"{indent}{_RULE}")
    for finding in active:
        label = _KIND_LABEL.get(finding.kind, finding.kind)
        click.echo(
            f"{indent}  "
            + click.style(f"[{label}]", fg="red", bold=True)
            + f" Line {finding.line}: {finding.snippet}"
        )
        if finding.callee:
            click.echo(f"{indent}    Callee: {finding.callee}")
        if finding.kind == "taint_flow" and finding.source:
            click.echo(
                f"{indent}    Source: {finding.source}  →  Sink: {finding.sink}"
            )

    if acknowledged:
        click.echo()
        click.echo(f"{indent}Acknowledged risks (.topos.toml)")
        click.echo(f"{indent}{_RULE}")
        for finding, entry in acknowledged:
            scope = getattr(entry, "scope", "**")
            reason = getattr(entry, "reason", "")
            scope_note = "" if scope in ("", "**", "*") else f"  [scope: {scope}]"
            click.echo(
                f"{indent}  "
                + click.style("[Allowed]", fg="yellow")
                + f" {finding.callee} (line {finding.line}) — "
                + click.style(f'reason: "{reason}"', dim=True)
                + scope_note
            )


def render_verdict_line(verdict: AdjustedVerdict, *, indent: str = "") -> None:
    """Print the raw→adjusted SECURE transition and grade-cap note."""
    if not verdict.suppressions_active:
        return
    raw = "PASS" if verdict.raw_secure_pass else "FAIL"
    adjusted = "PASS" if verdict.adjusted_secure_pass else "FAIL"
    click.echo()
    click.echo(
        f"{indent}secure: "
        + click.style(f"{raw} (raw)", fg="green" if verdict.raw_secure_pass else "red")
        + " → "
        + click.style(
            f"{adjusted} (acknowledged)",
            fg="green" if verdict.adjusted_secure_pass else "red",
            bold=True,
        )
    )
    if verdict.grade_capped:
        el: EvaluationValue = verdict.adjusted_element
        click.echo(
            f"{indent}"
            + click.style(
                f"Max grade capped: {el.symbol} {el.name} "
                "(acknowledged security risk — Gold/IDEAL unreachable)",
                fg="yellow",
            )
        )


def render_suggestions(suggestions: list[Suggestion], *, indent: str = "") -> None:
    """Print the Suggestions / Next Steps section, grouped by pillar."""
    if not suggestions:
        return
    click.echo()
    click.echo(f"{indent}Suggestions")
    click.echo(f"{indent}{_RULE}")
    for pillar in ("secure", "simple", "composable", "coverage"):
        group = [s for s in suggestions if s.pillar == pillar]
        for s in group:
            tag = click.style(f"[{s.severity}]", fg="cyan")
            click.echo(f"{indent}  {tag} {click.style(pillar, dim=True)}: {s.message}")


def suggestions_for(
    result: ClassificationResult, active: list[SecurityFinding]
) -> list[Suggestion]:
    """Convenience wrapper used by the CLI commands."""
    return suggest_refactors(result, active_findings=active)


def finding_to_dict(finding: SecurityFinding) -> dict[str, object]:
    return finding.model_dump()


def suggestion_to_dict(suggestion: Suggestion) -> dict[str, object]:
    return {
        "pillar": suggestion.pillar,
        "metric": suggestion.metric,
        "severity": suggestion.severity,
        "message": suggestion.message,
    }


def acknowledged_to_dict(
    acknowledged: list[tuple[SecurityFinding, object]],
) -> list[dict[str, object]]:
    out: list[dict[str, object]] = []
    for finding, entry in acknowledged:
        out.append(
            {
                "callee": finding.callee,
                "kind": finding.kind,
                "line": finding.line,
                "snippet": finding.snippet,
                "reason": getattr(entry, "reason", ""),
                "scope": getattr(entry, "scope", "**"),
            }
        )
    return out
