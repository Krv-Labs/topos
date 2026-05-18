"""
Dangerous-API reachability probe (CPG → ℝ).

Counts call-site nodes whose callee text matches the per-language
registry of dangerous APIs.  The match is intentionally textual: the
UAST mappers do not carry token text, so we slice the original source
by the CPG node's byte span and pattern-match the result.
"""

from __future__ import annotations

import re
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from topos.graphs.cpg.object import CodePropertyGraph

# Per-language registry of forbidden symbol names.  Conservative — meant
# to flag the obvious footguns, not to compete with a full SAST.
DANGEROUS_APIS: dict[str, set[str]] = {
    "python": {
        "eval",
        "exec",
        "compile",
        "pickle.loads",
        "yaml.load",
        "marshal.loads",
        "subprocess.call",
        "subprocess.Popen",
        "subprocess.run",
        "os.system",
        "os.popen",
        "__import__",
    },
    "javascript": {
        "eval",
        "Function",
        "setTimeout",
        "setInterval",
        "innerHTML",
        "document.write",
        "child_process.exec",
    },
    "typescript": {
        "eval",
        "Function",
        "innerHTML",
        "document.write",
        "child_process.exec",
    },
    "rust": {
        "unsafe",
        "transmute",
        "from_raw",
    },
    "cpp": {
        "gets",
        "strcpy",
        "strcat",
        "sprintf",
        "scanf",
        "system",
    },
}

_CALL_PREFIX = re.compile(r"^([A-Za-z_][A-Za-z0-9_.]*)\s*\(")


def dangerous_api_reachable(cpg: CodePropertyGraph) -> int:
    """
    Count CallExpr nodes whose callee text matches the dangerous-API
    registry for ``cpg.language``.  Matches both bare names (``eval``)
    and dotted/qualified names (``pickle.loads``).
    """
    registry = DANGEROUS_APIS.get(cpg.language, set())
    if not registry:
        return 0

    count = 0
    for node in cpg.nodes.values():
        if node.kind != "CallExpr":
            continue
        text = cpg.node_text(node)
        if not text:
            continue
        callee = _callee_from_text(text)
        if not callee:
            continue
        if _matches_registry(callee, registry):
            count += 1
    return count


def _callee_from_text(text: str) -> str:
    """Extract the dotted callee prefix from a call expression's text."""
    match = _CALL_PREFIX.match(text.strip())
    return match.group(1) if match else ""


def _matches_registry(callee: str, registry: set[str]) -> bool:
    if callee in registry:
        return True
    # Suffix match for qualified names: matches `foo.eval` against `eval`
    # and `mypkg.pickle.loads` against `pickle.loads`.
    return any(
        callee.endswith("." + api) or callee.endswith(api)
        for api in registry
        if "." in api or len(api) > 3
    )
