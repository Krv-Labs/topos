"""Tests for the Sighthound adapter — fixtures match real CLI JSON.

Real Sighthound findings use vulnerability labels as ``finding_type``
(e.g. ``"Command Injection"``), not rule mode. Taint findings are identified
by tags ``taint_analysis`` / ``data_flow`` / ``cross_file``. ``source_info``
uses ``context`` (not ``snippet``); ``sink_info`` uses ``function_name``.
"""

from __future__ import annotations

import json
from unittest.mock import MagicMock, patch

from topos.graphs.cpg.object import CodePropertyGraph
from topos.graphs.uast.models import NativeRef, SourceSpan, UASTNode
from topos.mcp.security_findings import security_findings
from topos.utils.sighthound import (
    count_findings,
    finding_callee,
    finding_sink_text,
    finding_source_text,
    is_taint_finding,
    run_sighthound_scan,
)

# Captured shape from live `sighthound --output-format json` on a vuln fixture.
# Search-mode findings: rule tags only (no taint_analysis).
MOCK_SEARCH_FINDING = {
    "file": "/tmp/topos-dogfood/vuln.py",
    "line": 5,
    "column": 5,
    "end_line": 5,
    "end_column": 21,
    "function": "eval",
    "finding_type": "Code Injection",
    "snippet": "eval(user_input)",
    "severity": "Critical",
    "confidence": "High",
    "description": "Use of eval()/exec() can lead to arbitrary code execution",
    "cwe_id": "cwe-95",
    "source_info": {
        "source_type": "User Input",
        "location": "Line 5",
        "context": "handle",
    },
    "sink_info": {
        "sink_type": "General Sink",
        "function_name": "eval",
        "location": "Line 5",
        "variable": "eval",
    },
    "tags": ["code", "injection", "eval"],
}

MOCK_SEARCH_FINDING_CMD = {
    "file": "/tmp/topos-dogfood/vuln.py",
    "line": 7,
    "column": 5,
    "end_line": 7,
    "end_column": 43,
    "function": "subprocess.run",
    "finding_type": "Command Injection",
    "snippet": "subprocess.run(user_input, shell=True)",
    "severity": "High",
    "confidence": "Medium",
    "description": "subprocess invoked with shell=True",
    "cwe_id": "cwe-78",
    "source_info": {
        "source_type": "User Input",
        "location": "Line 7",
        "context": "handle",
    },
    "sink_info": {
        "sink_type": "Command Execution",
        "function_name": "subprocess.run",
        "location": "Line 7",
        "variable": "subprocess",
    },
    "tags": ["command", "injection", "subprocess"],
}

MOCK_SIGHTHOUND_ONLY_FINDING = {
    **MOCK_SEARCH_FINDING,
    "line": 9,
    "function": "database.query",
    "finding_type": "SQL Injection",
    "snippet": "database.query(user_input)",
    "tags": ["sql", "injection"],
}

# Single-file taint finding tags from scanning_logic.rs.
MOCK_TAINT_FINDING = {
    "file": "app.py",
    "line": 42,
    "column": 8,
    "end_line": 42,
    "end_column": 30,
    "function": "os.system",
    "finding_type": "Command Injection",
    "snippet": "os.system(cmd)",
    "severity": "High",
    "confidence": "High",
    "description": "Tainted data reaches command execution sink",
    "cwe_id": "cwe-78",
    "source_info": {
        "source_type": "request.args",
        "location": "app.py:10",
        "context": "function: index",
    },
    "sink_info": {
        "sink_type": "os.system",
        "function_name": "os.system",
        "location": "app.py:42",
        "variable": "cmd",
    },
    "tags": ["taint_analysis", "data_flow"],
}

# Cross-file taint tags from multifile_taint.rs.
MOCK_CROSS_FILE_TAINT = {
    "file": "sink.py",
    "line": 8,
    "column": 0,
    "end_line": 8,
    "end_column": 0,
    "function": "run",
    "finding_type": "Command Injection",
    "snippet": "Sink: os.system",
    "severity": "High",
    "confidence": "High",
    "description": "Verified cross-file taint flow",
    "source_info": {
        "source_type": "input()",
        "location": "source.py:3",
        "context": "function: main",
    },
    "sink_info": {
        "sink_type": "os.system",
        "function_name": "run",
        "location": "sink.py:8",
        "variable": "cmd",
    },
    "tags": ["taint_analysis", "cross_file"],
}

MOCK_SIGHTHOUND_OUTPUT = [
    MOCK_SEARCH_FINDING,
    MOCK_SEARCH_FINDING_CMD,
    MOCK_TAINT_FINDING,
]


def test_is_taint_finding_uses_tags_not_finding_type():
    assert not is_taint_finding(MOCK_SEARCH_FINDING)
    assert not is_taint_finding(MOCK_SEARCH_FINDING_CMD)
    assert is_taint_finding(MOCK_TAINT_FINDING)
    assert is_taint_finding(MOCK_CROSS_FILE_TAINT)
    # finding_type alone must not classify search findings as taint
    assert not is_taint_finding(
        {"finding_type": "Command Injection", "tags": ["command"]}
    )
    # fallback when tags missing
    assert is_taint_finding({"finding_type": "Taint Flow"})
    assert not is_taint_finding({"finding_type": "Code Injection"})


def test_count_findings_real_schema():
    dangerous, taint = count_findings(MOCK_SIGHTHOUND_OUTPUT)
    assert dangerous == 2
    assert taint == 1
    # All-search payload (live vuln.py shape) must not zero out
    dangerous_only, taint_only = count_findings(
        [MOCK_SEARCH_FINDING, MOCK_SEARCH_FINDING_CMD]
    )
    assert dangerous_only == 2
    assert taint_only == 0
    d, t = count_findings([MOCK_CROSS_FILE_TAINT])
    assert (d, t) == (0, 1)
    assert count_findings(MOCK_SIGHTHOUND_OUTPUT, {"eval"}) == (1, 1)
    assert count_findings([MOCK_CROSS_FILE_TAINT], {"os.system"}) == (0, 0)


def test_finding_callee():
    assert finding_callee(MOCK_SEARCH_FINDING) == "eval"
    assert finding_callee(MOCK_CROSS_FILE_TAINT) == "os.system"
    assert finding_sink_text(MOCK_CROSS_FILE_TAINT) == "os.system"
    assert finding_callee({"sink_info": {"function_name": "exec"}}) == "exec"
    assert finding_callee({}) is None


def test_finding_source_text_keeps_cross_file_location():
    # Cross-file taint: origin lives in another file, so location must survive.
    assert (
        finding_source_text(MOCK_CROSS_FILE_TAINT)
        == "input() @ source.py:3 (function: main)"
    )
    # Single-file taint keeps type, location, and context too.
    assert (
        finding_source_text(MOCK_TAINT_FINDING)
        == "request.args @ app.py:10 (function: index)"
    )
    # Falls back to whichever field is present.
    assert finding_source_text({"source_info": {"location": "x.py:1"}}) == "x.py:1"
    assert finding_source_text({"source_info": {"context": "f"}}) == "f"
    assert finding_source_text({"source_info": {}}) is None
    assert finding_source_text({}) is None


@patch("subprocess.run")
def test_run_sighthound_scan_file(mock_run, tmp_path):
    mock_response = MagicMock()
    mock_response.stdout = json.dumps(MOCK_SIGHTHOUND_OUTPUT)
    mock_response.returncode = 0
    mock_run.return_value = mock_response

    target_file = tmp_path / "main.py"
    target_file.write_text("print(1)", encoding="utf-8")

    findings = run_sighthound_scan("print(1)", "python", target_file)
    assert len(findings) == 3
    mock_run.assert_called_once_with(
        ["sighthound", "--output-format", "json", str(target_file)],
        capture_output=True,
        text=True,
        check=False,
    )


@patch("subprocess.run")
def test_run_sighthound_scan_source_in_memory(mock_run):
    mock_response = MagicMock()
    mock_response.stdout = json.dumps(MOCK_SIGHTHOUND_OUTPUT)
    mock_response.returncode = 0
    mock_run.return_value = mock_response

    findings = run_sighthound_scan("eval(x)", "python")
    assert len(findings) == 3
    assert mock_run.call_count == 1
    args, _kwargs = mock_run.call_args
    assert args[0][0] == "sighthound"
    assert args[0][1] == "--output-format"
    assert args[0][2] == "json"
    assert args[0][3].endswith(".py")


@patch("subprocess.run")
def test_run_sighthound_scan_unwraps_findings_wrapper(mock_run, tmp_path):
    mock_response = MagicMock()
    mock_response.stdout = json.dumps({"findings": [MOCK_SEARCH_FINDING]})
    mock_response.returncode = 0
    mock_run.return_value = mock_response
    target = tmp_path / "x.py"
    target.write_text("x=1", encoding="utf-8")
    findings = run_sighthound_scan("x=1", "python", target)
    assert len(findings) == 1
    assert findings[0]["function"] == "eval"


@patch("subprocess.run")
def test_run_sighthound_scan_drops_non_dict_entries(mock_run, tmp_path):
    mock_response = MagicMock()
    mock_response.stdout = json.dumps([MOCK_SEARCH_FINDING, "bogus", None, 42])
    mock_response.returncode = 0
    mock_run.return_value = mock_response
    target = tmp_path / "x.py"
    target.write_text("x=1", encoding="utf-8")
    findings = run_sighthound_scan("x=1", "python", target)
    assert len(findings) == 1
    assert findings[0]["function"] == "eval"


@patch("shutil.which")
@patch("topos.utils.sighthound.run_sighthound_scan")
def test_cpg_metrics_uses_sighthound_when_present(mock_scan, mock_which):
    mock_which.return_value = "/usr/local/bin/sighthound"
    mock_scan.return_value = MOCK_SIGHTHOUND_OUTPUT

    span = SourceSpan("main.py", 0, 10, 1, 0, 1, 10)
    native = NativeRef("tree-sitter", "0.22", "module")
    uast = UASTNode("module", "python", span, native)
    cpg = CodePropertyGraph.from_uast(uast, source="print(1)")

    metrics = cpg.metrics()
    assert metrics["cpg.dangerous_calls"] == 2.0
    assert metrics["cpg.taint_flows"] == 1.0
    mock_scan.assert_called_once_with("print(1)", "python", "main.py")


@patch("shutil.which")
@patch("topos.utils.sighthound.run_sighthound_scan")
def test_security_findings_with_sighthound(mock_scan, mock_which):
    mock_which.return_value = "/usr/local/bin/sighthound"
    mock_scan.return_value = MOCK_SIGHTHOUND_OUTPUT

    span = SourceSpan("main.py", 0, 10, 1, 0, 1, 10)
    native = NativeRef("tree-sitter", "0.22", "module")
    uast = UASTNode("module", "python", span, native)
    cpg = CodePropertyGraph.from_uast(uast, source="print(1)")

    findings = security_findings(cpg)
    assert len(findings) == 3

    f1 = findings[0]
    assert f1.kind == "dangerous_call"
    assert f1.line == 5
    assert f1.snippet == "eval(user_input)"
    assert f1.callee == "eval"
    assert f1.source is None
    assert f1.sink is None

    f2 = findings[1]
    assert f2.kind == "dangerous_call"
    assert f2.callee == "subprocess.run"

    f3 = findings[2]
    assert f3.kind == "taint_flow"
    assert f3.line == 42
    assert f3.snippet == "os.system(cmd)"
    assert f3.callee == "os.system"
    # Source text keeps the origin location alongside type and context.
    assert f3.source == "request.args @ app.py:10 (function: index)"
    assert f3.sink == "os.system"


@patch("shutil.which")
@patch("topos.utils.sighthound.run_sighthound_scan")
def test_sighthound_allowlist_preserves_unrelated_rules(mock_scan, mock_which):
    mock_which.return_value = "/usr/local/bin/sighthound"
    mock_scan.return_value = [
        MOCK_SEARCH_FINDING,
        MOCK_SIGHTHOUND_ONLY_FINDING,
        MOCK_CROSS_FILE_TAINT,
    ]

    span = SourceSpan("main.py", 0, 10, 1, 0, 1, 10)
    native = NativeRef("tree-sitter", "0.22", "module")
    uast = UASTNode("module", "python", span, native)
    cpg = CodePropertyGraph.from_uast(uast, source="eval(x)")

    assert cpg.metrics()["cpg.dangerous_calls"] == 2.0
    adjusted = cpg.security_metrics(allow={"eval"})
    assert adjusted["cpg.dangerous_calls"] == 1.0
    assert adjusted["cpg.taint_flows"] == 1.0
    findings = security_findings(cpg, allow={"eval"})
    assert [finding.callee for finding in findings] == [
        "database.query",
        "os.system",
    ]
    assert findings[1].sink == "os.system"


@patch("shutil.which")
@patch("topos.utils.sighthound.run_sighthound_scan")
def test_security_findings_live_search_only_shape(mock_scan, mock_which):
    """Live vuln.py-style payload: all search findings, non-zero dangerous_calls."""
    mock_which.return_value = "/usr/local/bin/sighthound"
    mock_scan.return_value = [MOCK_SEARCH_FINDING, MOCK_SEARCH_FINDING_CMD]

    span = SourceSpan("vuln.py", 0, 10, 1, 0, 1, 10)
    native = NativeRef("tree-sitter", "0.22", "module")
    uast = UASTNode("module", "python", span, native)
    cpg = CodePropertyGraph.from_uast(uast, source="eval(x)")

    assert cpg.metrics()["cpg.dangerous_calls"] == 2.0
    assert cpg.metrics()["cpg.taint_flows"] == 0.0
    findings = security_findings(cpg)
    assert all(f.kind == "dangerous_call" for f in findings)
    assert {f.callee for f in findings} == {"eval", "subprocess.run"}
