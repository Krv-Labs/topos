from __future__ import annotations

import json
import subprocess
import tempfile
from pathlib import Path
from typing import Any

def run_sighthound_scan(
    source: str,
    language: str,
    file_path: str | Path | None = None
) -> list[dict[str, Any]]:
    """
    Run Sighthound SAST scanner on a source file or an in-memory source string.
    Returns a list of parsed JSON findings.
    """
    if file_path and Path(file_path).exists():
        return _run_cli(Path(file_path))
    
    # In-memory source: write to temporary file
    suffix_map = {
        "python": ".py",
        "rust": ".rs",
        "javascript": ".js",
        "typescript": ".ts",
        "cpp": ".cpp",
        "go": ".go",
    }
    suffix = suffix_map.get(language.lower(), ".py")
    with tempfile.NamedTemporaryFile(mode="w", suffix=suffix, delete=False, encoding="utf-8") as f:
        f.write(source)
        temp_path = Path(f.name)
    try:
        return _run_cli(temp_path)
    finally:
        try:
            temp_path.unlink()
        except OSError:
            pass

def _run_cli(target_path: Path) -> list[dict[str, Any]]:
    """Execute sighthound --output-format json and return parsed findings."""
    try:
        result = subprocess.run(
            ["sighthound", "--output-format", "json", str(target_path)],
            capture_output=True,
            text=True,
            check=False
        )
        if not result.stdout.strip():
            return []
        try:
            return json.loads(result.stdout)
        except json.JSONDecodeError:
            return []
    except FileNotFoundError:
        # sighthound CLI is not installed / in PATH
        return []

def count_findings(findings: list[dict[str, Any]]) -> tuple[int, int]:
    """Count the number of dangerous calls (search mode) and taint flows (taint mode)."""
    dangerous_calls = 0
    taint_flows = 0
    for finding in findings:
        ftype = finding.get("finding_type", "").lower()
        if ftype == "search":
            dangerous_calls += 1
        elif ftype == "taint":
            taint_flows += 1
    return dangerous_calls, taint_flows
