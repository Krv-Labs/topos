"""Adapter for the Corgea/Sighthound SAST CLI.

Sighthound's JSON schema (see ``Finding`` in Sighthound ``models.rs``):

* ``finding_type`` is a **vulnerability label** (e.g. ``"Command Injection"``),
  not the rule mode. Rule mode is ``search`` | ``taint`` on the rule, not the
  finding.
* Taint findings are tagged with ``taint_analysis`` (plus ``data_flow`` and/or
  ``cross_file``). Search findings carry rule tags only.
* ``source_info.context`` / ``sink_info.function_name`` — not ``snippet``.

This module is the single source of truth for invoking the CLI and classifying
findings into Topos SECURE metrics (``cpg.dangerous_calls`` /
``cpg.taint_flows``).
"""

from __future__ import annotations

import contextlib
import json
import subprocess
import tempfile
from pathlib import Path
from typing import Any

# Tags Sighthound itself uses to count search vs taint findings
# (see vulnerability_scanner.rs / scanning_logic.rs / multifile_taint.rs).
_TAINT_TAGS = frozenset({"taint_analysis", "data_flow", "cross_file"})
# Rare fallbacks when tags are missing (legacy / partial payloads).
_TAINT_FINDING_TYPES = frozenset({"taint", "taint flow"})


def run_sighthound_scan(
    source: str, language: str, file_path: str | Path | None = None
) -> list[dict[str, Any]]:
    """Run Sighthound on a path or in-memory source; return parsed findings."""
    if file_path and Path(file_path).exists():
        return _run_cli(Path(file_path))

    suffix_map = {
        "python": ".py",
        "rust": ".rs",
        "javascript": ".js",
        "typescript": ".ts",
        "cpp": ".cpp",
        "go": ".go",
    }
    suffix = suffix_map.get(language.lower(), ".py")
    with tempfile.NamedTemporaryFile(
        mode="w", suffix=suffix, delete=False, encoding="utf-8"
    ) as f:
        f.write(source)
        temp_path = Path(f.name)
    try:
        return _run_cli(temp_path)
    finally:
        with contextlib.suppress(OSError):
            temp_path.unlink()


def _run_cli(target_path: Path) -> list[dict[str, Any]]:
    """Execute ``sighthound --output-format json`` and return findings."""
    try:
        result = subprocess.run(
            ["sighthound", "--output-format", "json", str(target_path)],
            capture_output=True,
            text=True,
            check=False,
        )
    except FileNotFoundError:
        return []

    if not result.stdout.strip():
        return []
    try:
        payload = json.loads(result.stdout)
    except json.JSONDecodeError:
        return []
    return _normalize_findings_payload(payload)


def _normalize_findings_payload(payload: Any) -> list[dict[str, Any]]:
    """Accept a bare list or a wrapper object with a ``findings`` key."""
    if isinstance(payload, list):
        return [f for f in payload if isinstance(f, dict)]
    if isinstance(payload, dict):
        findings = payload.get("findings")
        if isinstance(findings, list):
            return [f for f in findings if isinstance(f, dict)]
    return []


def finding_tags(finding: dict[str, Any]) -> list[str]:
    """Return lowercased tags from a finding, if any."""
    tags = finding.get("tags") or []
    if not isinstance(tags, list):
        return []
    return [t.lower() for t in tags if isinstance(t, str)]


def is_taint_finding(finding: dict[str, Any]) -> bool:
    """True when Sighthound produced this finding via taint analysis.

    Prefer tags (``taint_analysis`` / ``data_flow`` / ``cross_file``), matching
    Sighthound's own search/taint counters. Fall back to a few literal
    ``finding_type`` values only when tags are absent.
    """
    tags = finding_tags(finding)
    if tags and any(t in _TAINT_TAGS for t in tags):
        return True
    if tags:
        # Explicit non-taint tags present → search-mode finding.
        return False
    ftype = str(finding.get("finding_type") or "").strip().lower()
    return ftype in _TAINT_FINDING_TYPES


def count_findings(findings: list[dict[str, Any]]) -> tuple[int, int]:
    """Count ``(dangerous_calls, taint_flows)`` for Topos SECURE metrics.

    Search-mode findings → ``cpg.dangerous_calls``.
    Taint-mode findings → ``cpg.taint_flows``.
    """
    dangerous_calls = 0
    taint_flows = 0
    for finding in findings:
        if is_taint_finding(finding):
            taint_flows += 1
        else:
            dangerous_calls += 1
    return dangerous_calls, taint_flows


def finding_callee(finding: dict[str, Any]) -> str | None:
    """Best-effort callee / sink function name for allowlisting and display."""
    func = finding.get("function")
    if isinstance(func, str) and func.strip():
        return func.strip()
    sink = finding.get("sink_info")
    if isinstance(sink, dict):
        name = sink.get("function_name")
        if isinstance(name, str) and name.strip():
            return name.strip()
    return None


def _clean_str(value: Any) -> str | None:
    """Return a stripped non-empty string, or None."""
    if isinstance(value, str) and value.strip():
        return value.strip()
    return None


def finding_source_text(finding: dict[str, Any]) -> str | None:
    """Human-readable taint source text from ``source_info`` (not ``snippet``).

    Combines ``source_type`` with the origin ``location`` (kept for cross-file
    flows, where the source lives in another file) and the ``context``.
    """
    info = finding.get("source_info")
    if not isinstance(info, dict):
        return None
    source_type = _clean_str(info.get("source_type"))
    location = _clean_str(info.get("location"))
    context = _clean_str(info.get("context"))

    head = source_type or location or context
    if head is None:
        return None

    # Anchor on the source type, then append location and context when they
    # add information beyond the head.
    text = head
    if location and location != head:
        text = f"{text} @ {location}"
    if context and context != head:
        text = f"{text} ({context})"
    return text


def finding_sink_text(finding: dict[str, Any]) -> str | None:
    """Human-readable sink text from ``sink_info`` or the finding snippet."""
    sink = finding.get("sink_info")
    if isinstance(sink, dict):
        name = sink.get("function_name")
        if isinstance(name, str) and name.strip():
            return name.strip()
        sink_type = sink.get("sink_type")
        if isinstance(sink_type, str) and sink_type.strip():
            return sink_type.strip()
    snippet = finding.get("snippet")
    if isinstance(snippet, str) and snippet.strip():
        return snippet.strip()
    return None


def finding_line(finding: dict[str, Any]) -> int:
    """1-based line number, clamped for SecurityFinding validation."""
    try:
        line = int(finding.get("line") or 1)
    except (TypeError, ValueError):
        line = 1
    return max(1, line)


def finding_snippet(finding: dict[str, Any]) -> str:
    """Top-level code snippet (always present on real Sighthound findings)."""
    snippet = finding.get("snippet")
    if isinstance(snippet, str):
        return snippet
    return ""
