from __future__ import annotations

import json
from pathlib import Path
from unittest.mock import MagicMock, patch

import pytest

from topos.graphs.cpg.object import CodePropertyGraph
from topos.graphs.uast.models import SourceSpan, NativeRef, UASTNode
from topos.mcp.security_findings import security_findings
from topos.utils.sighthound import run_sighthound_scan, count_findings


MOCK_SIGHTHOUND_OUTPUT = [
    {
        "file": "main.py",
        "line": 10,
        "column": 5,
        "end_line": 10,
        "end_column": 15,
        "function": "eval",
        "finding_type": "search",
        "snippet": "eval(user_input)",
        "severity": "Critical",
        "confidence": "High",
        "description": "Use of dangerous function eval"
    },
    {
        "file": "main.py",
        "line": 15,
        "column": 8,
        "end_line": 15,
        "end_column": 30,
        "function": "subprocess.run",
        "finding_type": "taint",
        "snippet": "subprocess.run(cmd)",
        "severity": "High",
        "confidence": "Medium",
        "description": "Command injection vulnerability",
        "source_info": {
            "snippet": "user_input = input()"
        },
        "sink_info": {
            "snippet": "subprocess.run(cmd)",
            "function_name": "subprocess.run"
        }
    }
]


def test_count_findings():
    dangerous, taint = count_findings(MOCK_SIGHTHOUND_OUTPUT)
    assert dangerous == 1
    assert taint == 1


@patch("subprocess.run")
def test_run_sighthound_scan_file(mock_run, tmp_path):
    mock_response = MagicMock()
    mock_response.stdout = json.dumps(MOCK_SIGHTHOUND_OUTPUT)
    mock_response.returncode = 0
    mock_run.return_value = mock_response

    target_file = tmp_path / "main.py"
    target_file.write_text("print(1)", encoding="utf-8")

    findings = run_sighthound_scan("print(1)", "python", target_file)
    assert len(findings) == 2
    mock_run.assert_called_once_with(
        ["sighthound", "--output-format", "json", str(target_file)],
        capture_output=True,
        text=True,
        check=False
    )


@patch("subprocess.run")
def test_run_sighthound_scan_source_in_memory(mock_run):
    mock_response = MagicMock()
    mock_response.stdout = json.dumps(MOCK_SIGHTHOUND_OUTPUT)
    mock_response.returncode = 0
    mock_run.return_value = mock_response

    findings = run_sighthound_scan("eval(x)", "python")
    assert len(findings) == 2
    # Should create a temporary file and clean it up
    assert mock_run.call_count == 1
    args, kwargs = mock_run.call_args
    assert args[0][0] == "sighthound"
    assert args[0][1] == "--output-format"
    assert args[0][2] == "json"
    # Temp file should have .py suffix
    assert args[0][3].endswith(".py")


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
    assert metrics["cpg.dangerous_calls"] == 1.0
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
    assert len(findings) == 2
    
    # Verify Search-mode finding
    f1 = findings[0]
    assert f1.kind == "dangerous_call"
    assert f1.line == 10
    assert f1.snippet == "eval(user_input)"
    assert f1.callee == "eval"
    
    # Verify Taint-mode finding
    f2 = findings[1]
    assert f2.kind == "taint_flow"
    assert f2.line == 15
    assert f2.snippet == "subprocess.run(cmd)"
    assert f2.callee == "subprocess.run"
    assert f2.source == "user_input = input()"
    assert f2.sink == "subprocess.run(cmd)"
