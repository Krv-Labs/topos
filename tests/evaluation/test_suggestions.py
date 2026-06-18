from __future__ import annotations

from topos.core.morphism import ProgramMorphism
from topos.evaluation.policies import Priority
from topos.evaluation.suggestions import suggest_refactors
from topos.mcp.evaluation import classify_code_string
from topos.mcp.security_findings import security_findings


def _classify(code: str):
    return classify_code_string(code, "python", Priority.SECURE)


def test_eval_yields_secure_fix_naming_callee() -> None:
    code = "def f(x):\n    return eval(x)\n"
    result = _classify(code)
    cpg = ProgramMorphism(source=code, language="python").build_cpg()
    findings = security_findings(cpg)

    suggestions = suggest_refactors(result, active_findings=findings)

    secure = [s for s in suggestions if s.pillar == "secure"]
    assert secure, "expected a SECURE suggestion for eval()"
    assert secure[0].severity == "fix"
    assert "eval" in secure[0].message


def test_high_complexity_yields_simple_suggestion() -> None:
    body = "\n".join(f"    if x == {i}:\n        x += {i}" for i in range(40))
    code = f"def f(x):\n{body}\n    return x\n"
    result = _classify(code)

    suggestions = suggest_refactors(result)

    simple = [s for s in suggestions if s.pillar == "simple"]
    assert simple, "expected a SIMPLE suggestion for a high-complexity function"
    assert all(s.severity == "fix" for s in simple)


def test_clean_simple_file_yields_no_simple_or_secure_fix() -> None:
    code = "def add(a, b):\n    return a + b\n"
    result = _classify(code)

    suggestions = suggest_refactors(result, active_findings=[])

    assert not [s for s in suggestions if s.pillar == "secure"]
    # A trivial clean function should not trigger complexity/branching fixes.
    assert not [s for s in suggestions if s.metric == "cfg.cyclomatic"]


def test_allowlisted_finding_produces_no_secure_suggestion() -> None:
    # When the eval finding is acknowledged upstream, active_findings is empty.
    code = "def f(x):\n    return eval(x)\n"
    result = _classify(code)

    suggestions = suggest_refactors(result, active_findings=[])

    assert not [s for s in suggestions if s.pillar == "secure"]
