"""Smoke tests for MCP documentation bundled in frozen binaries."""

from __future__ import annotations

import os
import subprocess
import sys
from pathlib import Path

import pytest

REPO_ROOT = Path(__file__).resolve().parents[2]
CONTENT_DIR = REPO_ROOT / "topos" / "mcp" / "resources" / "content"
DOC_TOPICS = (
    "agent-contract",
    "lattice",
    "metrics",
    "preferences",
    "priority",
    "workflows",
)


def test_mcp_doc_content_files_exist_in_repo() -> None:
    for topic in DOC_TOPICS:
        path = CONTENT_DIR / f"{topic}.md"
        assert path.is_file(), f"missing doc content file: {path}"


@pytest.mark.skipif(
    not os.environ.get("TOPOS_BINARY"),
    reason="Set TOPOS_BINARY to smoke-test MCP docs in a PyInstaller binary",
)
def test_frozen_binary_topos_get_doc_workflows() -> None:
    """Invoke topos_get_doc against a built binary via MCP stdio."""
    binary = os.environ["TOPOS_BINARY"]
    script = r"""
import json
import os
import subprocess
import sys

binary = os.environ["TOPOS_BINARY"]

def send(proc, payload):
    proc.stdin.write(json.dumps(payload) + "\n")
    proc.stdin.flush()

proc = subprocess.Popen(
    [binary, "mcp"],
    stdin=subprocess.PIPE,
    stdout=subprocess.PIPE,
    stderr=subprocess.PIPE,
    text=True,
    bufsize=1,
)

send(
    proc,
    {
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "topos-packaging-test", "version": "1.0"},
        },
    },
)
send(proc, {"jsonrpc": "2.0", "method": "notifications/initialized", "params": {}})
send(
    proc,
    {
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {"name": "topos_get_doc", "arguments": {"topic": "workflows"}},
    },
)

lines = []
for _ in range(20):
    line = proc.stdout.readline()
    if not line:
        break
    lines.append(line.strip())

proc.terminate()
proc.wait(timeout=5)

payloads = []
for line in lines:
    try:
        payloads.append(json.loads(line))
    except json.JSONDecodeError:
        continue

responses = [p for p in payloads if p.get("id") == 2]
assert responses, f"no tools/call response in MCP stdout: {lines!r}"
result = responses[-1].get("result") or {}
text = ""
if isinstance(result, dict):
    content = result.get("content") or []
    if content and isinstance(content[0], dict):
        text = content[0].get("text") or ""
    else:
        text = result.get("text") or ""
assert "workflows" in text.lower() or "review" in text.lower(), text[:200]
"""
    completed = subprocess.run(
        [sys.executable, "-c", script],
        env={**os.environ, "TOPOS_BINARY": binary},
        capture_output=True,
        text=True,
        timeout=120,
    )
    assert completed.returncode == 0, completed.stdout + completed.stderr
