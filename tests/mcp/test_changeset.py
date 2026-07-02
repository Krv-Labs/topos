"""Tests for topos_assess_changeset (issue #68)."""

from __future__ import annotations

import subprocess
from pathlib import Path

from topos.mcp.schemas import AssessChangesetInput, ChangesetResult
from topos.mcp.tools.assess.changeset import (
    _is_complexity_relocated,
    topos_assess_changeset,
)


def _changeset(tool_result) -> ChangesetResult:
    return ChangesetResult.model_validate(tool_result.structured_content)


def _git(cwd: Path, *args: str) -> None:
    subprocess.run(["git", "-C", str(cwd), *args], check=True, capture_output=True)


def _init_repo(root: Path) -> None:
    _git(root, "init")
    _git(root, "config", "user.email", "t@t.t")
    _git(root, "config", "user.name", "t")


def _use_root(root: Path, monkeypatch) -> None:
    from topos.mcp import security

    monkeypatch.setenv("TOPOS_MCP_FILE_ROOT", str(root))
    security.reset_file_root_cache()


# A multi-function module that passes the SIMPLE generator verdict (a trivial
# one-liner is degenerate and scores SLOP, so the baseline needs real body).
_CLEAN = (
    "def add(a, b):\n"
    "    total = a + b\n"
    "    return total\n\n\n"
    "def scale(values, factor):\n"
    "    result = []\n"
    "    for v in values:\n"
    "        result.append(v * factor)\n"
    "    return result\n\n\n"
    "def summarize(values):\n"
    "    return sum(values) / len(values)\n"
)
# Deeply branchy body that clearly fails the SIMPLE generator verdict.
_VERY_COMPLEX = "def f(x):\n" + "    if x: pass\n" * 25


def test_changeset_improvement_across_files(tmp_path, monkeypatch) -> None:
    _init_repo(tmp_path)
    a = tmp_path / "a.py"
    a.write_text(_VERY_COMPLEX, encoding="utf-8")
    _git(tmp_path, "add", "a.py")
    _git(tmp_path, "commit", "-m", "init")
    _use_root(tmp_path, monkeypatch)

    # Edit a.py to be clean and add a brand-new b.py (no baseline at HEAD).
    a.write_text(_CLEAN, encoding="utf-8")
    (tmp_path / "b.py").write_text(_CLEAN, encoding="utf-8")

    r = _changeset(topos_assess_changeset(AssessChangesetInput(files=["a.py", "b.py"])))
    assert r.error is None
    assert {e.filepath for e in r.files} == {"a.py", "b.py"}
    new_entry = next(e for e in r.files if e.filepath == "b.py")
    assert new_entry.is_new is True
    assert new_entry.baseline_verdict is None
    assert r.project_regression is False


def test_changeset_flags_complexity_relocation_metric() -> None:
    assert _is_complexity_relocated(
        {"ast.max_function_complexity": -3.0, "cfg.cyclomatic": 2.0}
    )
    assert not _is_complexity_relocated(
        {"ast.max_function_complexity": -3.0, "cfg.cyclomatic": -1.0}
    )
    assert not _is_complexity_relocated({})


def test_changeset_detects_project_regression(tmp_path, monkeypatch) -> None:
    _init_repo(tmp_path)
    a = tmp_path / "a.py"
    a.write_text(_CLEAN, encoding="utf-8")
    _git(tmp_path, "add", "a.py")
    _git(tmp_path, "commit", "-m", "init")
    _use_root(tmp_path, monkeypatch)

    # Regress the file from clean to a body that fails SIMPLE.
    a.write_text(_VERY_COMPLEX, encoding="utf-8")

    r = _changeset(topos_assess_changeset(AssessChangesetInput(files=["a.py"])))
    assert r.project_regression is True
    assert "project_regression" in r.agent_contract.blocked_by
    assert r.agent_contract.next_tool == "topos_inspect_code"


def test_changeset_flags_invalid_gitnexus_dir(tmp_path, monkeypatch) -> None:
    _init_repo(tmp_path)
    a = tmp_path / "a.py"
    a.write_text(_CLEAN, encoding="utf-8")
    _git(tmp_path, "add", "a.py")
    _git(tmp_path, "commit", "-m", "init")
    _use_root(tmp_path, monkeypatch)
    a.write_text(_CLEAN + "\ndef extra():\n    return 2\n", encoding="utf-8")

    r = _changeset(
        topos_assess_changeset(
            AssessChangesetInput(
                files=["a.py"],
                gitnexus_dir=str(tmp_path / "missing"),
            )
        )
    )

    assert r.agent_contract is not None
    assert "invalid_gitnexus_dir" in r.agent_contract.blocked_by
    assert "missing_gitnexus_dir" not in r.agent_contract.blocked_by
    assert r.agent_contract.next_tool != "topos_generate_depgraph"


def test_changeset_rejects_invalid_baseline_ref(tmp_path, monkeypatch) -> None:
    # A mistyped ref must fail structurally, not masquerade as "every file is
    # new" (git can't tell an absent-at-ref path from an invalid ref).
    _init_repo(tmp_path)
    a = tmp_path / "a.py"
    a.write_text(_CLEAN, encoding="utf-8")
    _git(tmp_path, "add", "a.py")
    _git(tmp_path, "commit", "-m", "init")
    _use_root(tmp_path, monkeypatch)

    r = _changeset(
        topos_assess_changeset(
            AssessChangesetInput(files=["a.py"], baseline_ref="no-such-ref")
        )
    )
    assert r.error is not None
    assert "no-such-ref" in r.error
    assert r.files == []


def test_changeset_rejects_invalid_ref_when_root_is_above_repo(
    tmp_path, monkeypatch
) -> None:
    workspace = tmp_path / "workspace"
    repo = workspace / "repo"
    repo.mkdir(parents=True)
    _init_repo(repo)
    a = repo / "a.py"
    a.write_text(_CLEAN, encoding="utf-8")
    _git(repo, "add", "a.py")
    _git(repo, "commit", "-m", "init")
    _use_root(workspace, monkeypatch)

    r = _changeset(
        topos_assess_changeset(
            AssessChangesetInput(files=["repo/a.py"], baseline_ref="no-such-ref")
        )
    )

    assert r.error is not None
    assert "no-such-ref" in r.error
    assert r.files == []


def test_changeset_markdown_has_file_table(tmp_path, monkeypatch) -> None:
    _init_repo(tmp_path)
    a = tmp_path / "a.py"
    a.write_text(_CLEAN, encoding="utf-8")
    _git(tmp_path, "add", "a.py")
    _git(tmp_path, "commit", "-m", "init")
    _use_root(tmp_path, monkeypatch)
    a.write_text(_CLEAN + "\ndef g():\n    return 2\n", encoding="utf-8")

    tr = topos_assess_changeset(AssessChangesetInput(files=["a.py"]))
    text = tr.content[0].text
    assert "## Files" in text
    assert "a.py" in text
