"""Tests for the consolidated security remediation guidance."""

from __future__ import annotations

import pytest
from topos.evaluation.security_guidance import (
    DEFAULT_OPERATIONS,
    REMEDIATIONS,
    TAINT_OPERATIONS,
    remediation_for,
)
from topos.functors.probes.cpg.danger import DANGEROUS_APIS, match_registry_key
from topos.mcp.schemas import SecurityFinding


def _finding(callee: str | None, kind: str = "dangerous_call") -> SecurityFinding:
    return SecurityFinding(kind=kind, line=1, snippet=f"{callee}(x)", callee=callee)


@pytest.mark.parametrize(
    ("language", "api"),
    [(lang, api) for lang, apis in DANGEROUS_APIS.items() for api in sorted(apis)],
)
def test_every_registry_entry_has_specific_guidance(language: str, api: str) -> None:
    """The dangerous-API registry can never silently outgrow the guidance table."""
    advice, operations = remediation_for(_finding(api))
    assert operations != DEFAULT_OPERATIONS, f"{language}:{api} fell to default"
    assert "Remove or sandbox" not in advice


def test_qualified_callee_suffix_matches() -> None:
    _, operations = remediation_for(_finding("mypkg.os.system"))
    assert operations == ("remove_shell_execution", "pass_argument_array")


@pytest.mark.parametrize(
    "api",
    sorted({api for apis in DANGEROUS_APIS.values() for api in apis}),
)
def test_probe_guidance_parity_for_qualified_callees(api: str) -> None:
    """Any qualified callee the probe would flag resolves to specific guidance.

    The probe suffix-matches, so `mypkg.<api>` is flagged for every registry
    entry; the guidance lookup must resolve the same callee to a non-default
    remediation or the two matchers have diverged.
    """
    qualified = f"mypkg.{api}"
    assert match_registry_key(qualified, DANGEROUS_APIS_UNION) is not None
    _, operations = remediation_for(_finding(qualified))
    assert operations != DEFAULT_OPERATIONS, qualified


DANGEROUS_APIS_UNION = sorted({api for apis in DANGEROUS_APIS.values() for api in apis})


def test_subprocess_popen_prefers_longest_key() -> None:
    """Longest-key matching: subprocess.popen must not match os.popen."""
    assert (
        match_registry_key("subprocess.popen", list(REMEDIATIONS)) == "subprocess.popen"
    )


def test_deserialization_apis_get_safe_deserializer_ops() -> None:
    for callee in ("pickle.loads", "yaml.load", "marshal.loads"):
        _, operations = remediation_for(_finding(callee))
        assert operations == ("use_safe_deserializer", "validate_input"), callee


def test_taint_flow_prose_and_operations() -> None:
    finding = SecurityFinding(
        kind="taint_flow",
        line=7,
        snippet="eval(data)",
        callee="eval",
        source="request.args",
        sink="eval(data)",
    )
    advice, operations = remediation_for(finding)
    assert "request.args" in advice
    assert "line 7" in advice
    assert operations == TAINT_OPERATIONS


def test_unknown_callee_falls_back_to_default() -> None:
    advice, operations = remediation_for(_finding("totally_benign_call"))
    assert operations == DEFAULT_OPERATIONS
    assert "Remove or sandbox" in advice
