"""
Security remediation guidance — one table for prose and operation tokens.

Maps each dangerous API in :data:`~topos.functors.probes.cpg.danger.DANGEROUS_APIS`
to a :class:`Remediation`: the imperative prose the suggestion engine renders
and the machine-readable operation tokens MCP refactor targets carry. Lookup
uses the probe's own suffix-aware matcher so a qualified or aliased callee
(``mypkg.os.system``, ``Popen``) resolves to the same guidance the probe
flagged it under.

``tests/evaluation/test_security_guidance.py`` guards that every registry
entry has a non-default remediation — the registry cannot silently outgrow
this table.
"""

from __future__ import annotations

from dataclasses import dataclass

from topos.functors.probes.cpg.danger import match_registry_key
from topos.mcp.schemas import SecurityFinding


@dataclass(frozen=True)
class Remediation:
    """Guidance for one dangerous API: prose advice + operation tokens."""

    advice: str
    operations: tuple[str, ...]


_DYNAMIC_EXEC = ("replace_dynamic_execution", "use_static_dispatch")
_SHELL = ("remove_shell_execution", "pass_argument_array")
_DESERIALIZE = ("use_safe_deserializer", "validate_input")
_DOM = ("sanitize_html", "build_dom_nodes")
_BOUNDED_COPY = ("use_bounded_copy",)
_UNSAFE = ("encapsulate_unsafe",)

TAINT_OPERATIONS: tuple[str, ...] = ("validate_input", "sanitize_before_sink")
DEFAULT_OPERATIONS: tuple[str, ...] = ("replace_dangerous_api", "validate_input")

# Keyed by dangerous callee (lowercase; suffix-matched via the danger probe's
# matcher). Prose preserved verbatim from the original suggestion engine.
REMEDIATIONS: dict[str, Remediation] = {
    "eval": Remediation(
        "Replace `eval` with `ast.literal_eval` or explicit parsing.",
        _DYNAMIC_EXEC,
    ),
    "exec": Remediation(
        "Remove `exec`; call the code path directly or dispatch via a map.",
        _DYNAMIC_EXEC,
    ),
    "compile": Remediation(
        "Avoid dynamic `compile`; use a static, reviewed code path.",
        _DYNAMIC_EXEC,
    ),
    "__import__": Remediation(
        "Import statically; avoid `__import__` on dynamic names.",
        _DYNAMIC_EXEC,
    ),
    "function": Remediation(
        "Avoid the `Function` constructor; call a known function directly.",
        _DYNAMIC_EXEC,
    ),
    "settimeout": Remediation(
        "Pass a function reference to `setTimeout`, never a string.",
        _DYNAMIC_EXEC,
    ),
    "setinterval": Remediation(
        "Pass a function reference to `setInterval`, never a string.",
        _DYNAMIC_EXEC,
    ),
    "pickle.loads": Remediation(
        "Use `json` or a schema-validated deserializer instead of `pickle`.",
        _DESERIALIZE,
    ),
    "marshal.loads": Remediation(
        "Avoid `marshal`; deserialize with `json` or a safe format.",
        _DESERIALIZE,
    ),
    "yaml.load": Remediation(
        "Use `yaml.safe_load` instead of `yaml.load`.",
        _DESERIALIZE,
    ),
    "os.system": Remediation(
        "Replace `os.system` with `subprocess.run([...])` (no shell).",
        _SHELL,
    ),
    "os.popen": Remediation(
        "Replace `os.popen` with `subprocess.run([...], capture_output=True)`.",
        _SHELL,
    ),
    "subprocess.call": Remediation(
        "Pass an argument list and avoid `shell=True`.",
        _SHELL,
    ),
    "subprocess.run": Remediation(
        "Pass an argument list and avoid `shell=True`.",
        _SHELL,
    ),
    "subprocess.popen": Remediation(
        "Pass an argument list and avoid `shell=True`.",
        _SHELL,
    ),
    "child_process.exec": Remediation(
        "Use `execFile`/`spawn` with an argument array (no shell).",
        _SHELL,
    ),
    "system": Remediation(
        "Replace `system()` with an `exec*`-family call (no shell).",
        _SHELL,
    ),
    "exec.command": Remediation(
        "Pass a fixed argument list to `exec.Command`; never build the "
        "command or its args from untrusted input.",
        _SHELL,
    ),
    "exec.commandcontext": Remediation(
        "Pass a fixed argument list to `exec.CommandContext`; never build "
        "the command or its args from untrusted input.",
        _SHELL,
    ),
    "os.startprocess": Remediation(
        "Avoid `os.StartProcess` with untrusted paths or args; validate "
        "and pass an explicit argument list.",
        _SHELL,
    ),
    "syscall.exec": Remediation(
        "Avoid `syscall.Exec`; validate the program path and argument "
        "list before replacing the process image.",
        _SHELL,
    ),
    "syscall.forkexec": Remediation(
        "Avoid `syscall.ForkExec`; validate the program path and "
        "argument list, or prefer `os/exec` with explicit arguments.",
        _SHELL,
    ),
    "innerhtml": Remediation(
        "Set text via `textContent`, or sanitize before assigning HTML.",
        _DOM,
    ),
    "document.write": Remediation(
        "Build DOM nodes instead of `document.write`.",
        _DOM,
    ),
    "strcpy": Remediation(
        "Use a bounded copy (`strncpy`/`snprintf`).",
        _BOUNDED_COPY,
    ),
    "strcat": Remediation(
        "Use a bounded concat (`strncat`/`snprintf`).",
        _BOUNDED_COPY,
    ),
    "sprintf": Remediation(
        "Use `snprintf` with an explicit buffer size.",
        _BOUNDED_COPY,
    ),
    "scanf": Remediation(
        "Use bounded input (`fgets` + parsing, or width-limited `scanf`).",
        _BOUNDED_COPY,
    ),
    "gets": Remediation(
        "Replace `gets` with `fgets` and an explicit length.",
        _BOUNDED_COPY,
    ),
    "transmute": Remediation(
        "Avoid `mem::transmute`; use a safe conversion or `bytemuck`.",
        _UNSAFE,
    ),
    "unsafe": Remediation(
        "Confine or remove the `unsafe` block; document the invariant it upholds.",
        _UNSAFE,
    ),
    "from_raw": Remediation(
        "Encapsulate `from_raw` behind a safe wrapper; document pointer ownership.",
        _UNSAFE,
    ),
}


def remediation_for(finding: SecurityFinding) -> tuple[str, tuple[str, ...]]:
    """(advice, operations) for a security finding.

    Taint flows get flow-specific prose; dangerous calls resolve through the
    suffix-aware registry matcher; anything unmatched gets generic guidance.
    """
    if finding.kind == "taint_flow":
        src = finding.source or "untrusted input"
        sink = finding.callee or finding.sink or "the dangerous call"
        return (
            f"Validate/sanitize `{src}` before it reaches `{sink}` "
            f"(line {finding.line}).",
            TAINT_OPERATIONS,
        )
    callee = (finding.callee or "").lower()
    key = match_registry_key(callee, list(REMEDIATIONS)) if callee else None
    if key is not None:
        entry = REMEDIATIONS[key]
        return entry.advice, entry.operations
    return (
        f"Remove or sandbox the dangerous call `{finding.callee}` "
        f"(line {finding.line}).",
        DEFAULT_OPERATIONS,
    )
