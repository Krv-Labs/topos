"""Security diagnostic overlay helpers for MCP tools."""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path

from topos.config import ToposConfig, load_topos_config, merge_cli_allows
from topos.core.morphism import ProgramMorphism
from topos.evaluation.characteristic_morphism import ClassificationResult
from topos.evaluation.suppression import AdjustedVerdict, apply_allowlist

from .schemas import AcknowledgedRisk, SecurityFinding
from .security_findings import security_findings


@dataclass(frozen=True)
class SecurityOverlay:
    """Allowlist-aware security diagnostics for one evaluation."""

    active_findings: list[SecurityFinding]
    acknowledged_risks: list[AcknowledgedRisk]
    verdict: AdjustedVerdict


def _secure_failed(result: ClassificationResult) -> bool:
    return bool(
        result.raw_metrics.get("cpg.dangerous_calls", 0.0) > 0
        or result.raw_metrics.get("cpg.taint_flows", 0.0) > 0
    )


def _config_for(path: Path | None, allows: list[str]) -> ToposConfig:
    config = load_topos_config(path) if path is not None else ToposConfig()
    return merge_cli_allows(config, tuple(allows))


def _acknowledged_to_models(
    acknowledged: list[tuple[SecurityFinding, object]],
) -> list[AcknowledgedRisk]:
    return [
        AcknowledgedRisk(
            callee=finding.callee,
            kind=finding.kind,
            line=finding.line,
            snippet=finding.snippet,
            reason=getattr(entry, "reason", ""),
            scope=getattr(entry, "scope", "**"),
        )
        for finding, entry in acknowledged
    ]


def overlay_for_file(
    path: Path,
    result: ClassificationResult,
    *,
    allows: list[str] | None = None,
    include_security_findings: bool,
) -> SecurityOverlay | None:
    """Apply the project/one-off allowlist over a file classification."""
    if not result.is_parseable:
        return None
    config = _config_for(path, allows or [])
    if not _secure_failed(result):
        return None

    from .evaluation import detect_language

    cpg = ProgramMorphism.from_file(path, language=detect_language(path)).build_cpg()
    findings = security_findings(cpg)
    verdict = apply_allowlist(result, findings, config, file_path=str(path), cpg=cpg)
    active = verdict.active_findings if include_security_findings else []
    return SecurityOverlay(
        active, _acknowledged_to_models(verdict.acknowledged), verdict
    )


def overlay_for_source(
    source: str,
    language: str,
    result: ClassificationResult,
    *,
    file_path: Path | None = None,
    allows: list[str] | None = None,
    include_security_findings: bool,
) -> SecurityOverlay | None:
    """Apply the project/one-off allowlist over an in-memory classification."""
    if not result.is_parseable:
        return None
    config = _config_for(file_path, allows or [])
    if not _secure_failed(result):
        return None

    cpg = ProgramMorphism(source=source, language=language).build_cpg()
    findings = security_findings(cpg)
    verdict = apply_allowlist(
        result,
        findings,
        config,
        file_path=str(file_path) if file_path is not None else None,
        cpg=cpg,
    )
    active = verdict.active_findings if include_security_findings else []
    return SecurityOverlay(
        active, _acknowledged_to_models(verdict.acknowledged), verdict
    )
