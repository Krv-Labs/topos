"""
Dangerous-API reachability probe (CPG → ℝ).

Counts call-site nodes whose callee text matches the per-language
registry of dangerous APIs.  The match is intentionally textual: the
UAST mappers do not carry token text, so we slice the original source
by the CPG node's byte span and pattern-match the result.
"""

from __future__ import annotations

import re
from collections.abc import Iterable
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


def effective_registry(language: str, allow: set[str] | None) -> set[str]:
    """Dangerous-API registry for *language* minus any allowlisted patterns.

    A registry entry is dropped when it matches an allowlist pattern under
    the same suffix-aware rules used for callee matching.  ``allow=None``
    (or empty) returns the full registry unchanged — the canonical default.
    """
    registry = DANGEROUS_APIS.get(language, set())
    if not allow:
        return registry
    return {api for api in registry if not _matches_registry(api, allow)}


def dangerous_api_reachable(
    cpg: CodePropertyGraph, allow: set[str] | None = None
) -> int:
    """
    Count CallExpr nodes whose callee text matches the dangerous-API
    registry for ``cpg.language``.  Matches both bare names (``eval``)
    and dotted/qualified names (``pickle.loads``).

    When *allow* is given, allowlisted patterns are excluded from the
    registry first.  The default ``allow=None`` preserves the canonical
    behavior used by :meth:`CodePropertyGraph.metrics`.
    """
    registry = effective_registry(cpg.language, allow)
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


def match_registry_key(callee: str, keys: Iterable[str]) -> str | None:
    """The registry key *callee* matches, or None.

    Exact membership wins; otherwise suffix match for qualified names
    (``foo.eval`` against ``eval``, ``mypkg.pickle.loads`` against
    ``pickle.loads``), restricted to dotted or longer-than-3-char keys to
    avoid spurious short-name suffix hits. Prefers the longest matching key
    so ``pickle.loads`` beats a hypothetical bare ``loads``.
    """
    candidates = list(keys)
    if callee in candidates:
        return callee
    best: str | None = None
    for key in candidates:
        if "." not in key and len(key) <= 3:
            continue
        matches = callee.endswith("." + key) or callee.endswith(key)
        if matches and (best is None or len(key) > len(best)):
            best = key
    return best


def _matches_registry(callee: str, registry: set[str]) -> bool:
    return match_registry_key(callee, registry) is not None
