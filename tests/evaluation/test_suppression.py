from __future__ import annotations

from pathlib import Path

from topos.config import AllowEntry, ToposConfig, load_topos_config, merge_cli_allows
from topos.core.morphism import ProgramMorphism
from topos.core.omega import EvaluationValue
from topos.evaluation.policies import Priority
from topos.evaluation.suppression import apply_allowlist
from topos.functors.probes.cpg.danger import dangerous_api_reachable
from topos.mcp.evaluation import classify_code_string
from topos.mcp.security_findings import security_findings

_EVAL = "def f(x):\n    return eval(x)\n"


def _setup(code: str):
    result = classify_code_string(code, "python", Priority.SECURE)
    cpg = ProgramMorphism(source=code, language="python").build_cpg()
    findings = security_findings(cpg)
    return result, cpg, findings


def test_allowlist_flips_secure_and_caps_grade() -> None:
    result, cpg, findings = _setup(_EVAL)
    config = ToposConfig(allow=[AllowEntry(pattern="eval", reason="trusted REPL")])

    verdict = apply_allowlist(result, findings, config, file_path="f.py", cpg=cpg)

    assert verdict.raw_secure_pass is False
    assert verdict.adjusted_secure_pass is True
    assert not verdict.active_findings
    assert len(verdict.acknowledged) == 1
    assert verdict.acknowledged[0][0].callee == "eval"
    # SECURE is only acknowledged, never a clean pass — no top grade.
    assert verdict.adjusted_element != EvaluationValue.IDEAL


def test_no_allowlist_leaves_raw_intact() -> None:
    result, cpg, findings = _setup(_EVAL)
    verdict = apply_allowlist(
        result, findings, ToposConfig(), file_path="f.py", cpg=cpg
    )

    assert verdict.raw_secure_pass is False
    assert verdict.adjusted_secure_pass is False
    assert len(verdict.active_findings) == 1
    assert not verdict.acknowledged


def test_probe_allow_none_is_canonical() -> None:
    # Regression guard: the canonical pipeline must be unaffected by the param.
    cpg = ProgramMorphism(source=_EVAL, language="python").build_cpg()
    assert dangerous_api_reachable(cpg) == 1
    assert dangerous_api_reachable(cpg, None) == 1
    assert dangerous_api_reachable(cpg, {"eval"}) == 0


def test_scope_limits_suppression(tmp_path: Path) -> None:
    result, cpg, findings = _setup(_EVAL)
    config = ToposConfig(
        allow=[AllowEntry(pattern="eval", reason="ok here", scope="experiments/**")],
        root=tmp_path,
    )

    in_scope = apply_allowlist(
        result, findings, config, file_path=str(tmp_path / "experiments/a.py"), cpg=cpg
    )
    out_scope = apply_allowlist(
        result, findings, config, file_path=str(tmp_path / "serving/a.py"), cpg=cpg
    )

    assert in_scope.adjusted_secure_pass is True
    assert out_scope.adjusted_secure_pass is False


def test_entry_without_reason_is_dropped(tmp_path: Path) -> None:
    cfg_file = tmp_path / ".topos.toml"
    cfg_file.write_text(
        '[[secure.allow]]\npattern = "eval"\n',  # missing reason
        encoding="utf-8",
    )
    config = load_topos_config(tmp_path)
    assert config.allow == []


def test_cli_allow_merge_adds_ephemeral_entry() -> None:
    config = merge_cli_allows(ToposConfig(), ("eval,yaml.load",))
    patterns = {e.pattern for e in config.allow}
    assert patterns == {"eval", "yaml.load"}
    assert all(e.reason for e in config.allow)
