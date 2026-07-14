from __future__ import annotations

import io
import json
import re
from pathlib import Path

import click
from click.testing import CliRunner
from topos.cli.main import cli


class TtyStringIO(io.StringIO):
    def isatty(self) -> bool:
        return True


def test_evaluate_no_paths():
    runner = CliRunner()
    result = runner.invoke(cli, ["evaluate"])
    assert result.exit_code != 0
    assert "Error: No paths provided." in result.output


def test_evaluate_file(tmp_path: Path):
    f = tmp_path / "test.py"
    f.write_text("x = 1\n", encoding="utf-8")

    runner = CliRunner()
    result = runner.invoke(cli, ["evaluate", str(f)])
    assert result.exit_code == 0
    assert str(f) in result.output
    assert "Files" in result.output

    clean_output = re.sub(r"\x1b\[[0-9;]*m", "", result.output)
    assert f"{f}  [" in clean_output
    assert "Directory Average Score" in result.output
    assert "Directory Floor Verdict" in result.output


def test_evaluate_recursive(tmp_path: Path):
    d = tmp_path / "src"
    d.mkdir()
    (d / "a.py").write_text("x = 1\n", encoding="utf-8")
    (d / "b.py").write_text("y = 2\n", encoding="utf-8")

    runner = CliRunner()
    result = runner.invoke(cli, ["evaluate", str(d), "-r"])
    assert result.exit_code == 0
    assert "Evaluated 2 files" in result.output
    assert "Files" in result.output
    assert "Pillars" not in result.output
    assert "a.py" in result.output
    assert "b.py" in result.output
    assert "Directory Average Score" in result.output
    assert "Directory Floor Verdict" in result.output


def test_evaluate_large_recursive_uses_summary(tmp_path: Path):
    d = tmp_path / "src"
    d.mkdir()
    for idx in range(6):
        (d / f"file_{idx}.py").write_text(f"x = {idx}\n", encoding="utf-8")

    runner = CliRunner()
    result = runner.invoke(cli, ["evaluate", str(d), "-r"])

    assert result.exit_code == 0
    assert "Evaluated 6 files" in result.output
    assert "Pillars" in result.output
    assert "Needs attention" in result.output
    assert "Best file" in result.output
    assert "Files" not in result.output
    assert "Directory Average Score" in result.output
    assert "Directory Floor Verdict" in result.output


def test_evaluate_json(tmp_path: Path):
    f = tmp_path / "test.py"
    f.write_text("x = 1\n", encoding="utf-8")

    runner = CliRunner()
    result = runner.invoke(cli, ["evaluate", str(f), "--json"])
    assert result.exit_code == 0
    import json

    data = json.loads(result.output)

    assert "results" in data
    assert data["results"][0]["file"] == str(f)
    assert "Directory Floor Verdict" not in result.output


def test_evaluate_progress_bar_for_interactive_text(tmp_path: Path, monkeypatch):
    d = tmp_path / "src"
    d.mkdir()
    (d / "a.py").write_text("x = 1\n", encoding="utf-8")
    (d / "b.py").write_text("y = 2\n", encoding="utf-8")

    progress_stream = TtyStringIO()
    original_get_text_stream = click.get_text_stream

    def get_text_stream(name: str):
        if name == "stderr":
            return progress_stream
        return original_get_text_stream(name)

    monkeypatch.setattr(click, "get_text_stream", get_text_stream)

    runner = CliRunner()
    result = runner.invoke(cli, ["evaluate", str(d), "-r"])

    assert result.exit_code == 0
    assert "Directory Floor Verdict" in result.output
    progress_output = progress_stream.getvalue()
    assert progress_output.startswith("\n")
    assert progress_output.endswith("\n\n")
    assert "Evaluating" in progress_output
    assert "█" in progress_output
    assert "░" in progress_output
    assert "%" not in progress_output
    assert "#" not in progress_output


def test_evaluate_lowest_hanging_fruit(tmp_path: Path):
    d = tmp_path / "src"
    d.mkdir()
    # Five hard failures on `simple` (score 24%, gap 36) via genuinely
    # redundant boilerplate — large enough (240 bytes) to be well above the
    # entropy probe's tiny-input size floor, so this fails on real
    # repetitiveness rather than a single-line file's fixed-overhead
    # artifact.
    for idx in range(5):
        (d / f"trivial_{idx}.py").write_text("x = 1\n" * 40, encoding="utf-8")
    # One clear near-miss on `simple` (55%, the smallest failing gap) via a
    # chain of 8 independent ifs — cyclomatic/max-function-complexity carry
    # the score down close to (but under) the 60% threshold, comfortably
    # sized (276 bytes) to be unaffected by the entropy size floor too.
    near = d / "near_miss.py"
    near_lines = ["def f(" + ", ".join(f"a{i}" for i in range(1, 9)) + "):"]
    for i in range(1, 9):
        near_lines.append(f"    if a{i}:")
        near_lines.append(f"        return {i}")
    near_lines.append("    return 0")
    near.write_text("\n".join(near_lines) + "\n", encoding="utf-8")

    runner = CliRunner()
    result = runner.invoke(cli, ["evaluate", str(d), "-r"])
    clean_output = re.sub(r"\x1b\[[0-9;]*m", "", result.output)

    assert result.exit_code == 0
    assert "Lowest-hanging fruit" in clean_output

    fruit_section = clean_output[clean_output.index("Lowest-hanging fruit") :]
    assert "near_miss.py" in fruit_section
    # The near-miss is the cheapest win: ranked first with the smallest gap.
    assert re.search(r"1\.\s+.*near_miss\.py", fruit_section)
    assert "simple 55% → 60%" in fruit_section


def test_evaluate_lowest_hanging_fruit_all_pass_message(tmp_path: Path):
    d = tmp_path / "src"
    d.mkdir()
    # A clean module that passes every measured pillar.
    clean = (
        '"""A small clean module."""\n\n\n'
        "def add(a, b):\n"
        '    """Return the sum."""\n'
        "    return a + b\n\n\n"
        "def mul(a, b):\n"
        '    """Return the product."""\n'
        "    return a * b\n"
    )
    for idx in range(6):
        (d / f"clean_{idx}.py").write_text(clean, encoding="utf-8")

    runner = CliRunner()
    result = runner.invoke(cli, ["evaluate", str(d), "-r"])
    clean_output = re.sub(r"\x1b\[[0-9;]*m", "", result.output)

    assert result.exit_code == 0
    assert "Lowest-hanging fruit" in clean_output
    assert "All measured pillars pass" in clean_output


def test_help_short_flag_root_and_subcommands():
    runner = CliRunner()
    for args in (["-h"], ["evaluate", "-h"], ["coverage", "-h"]):
        result = runner.invoke(cli, args)
        assert result.exit_code == 0, args
        assert "Usage:" in result.output, args


def test_evaluate_json_suppresses_progress_bar(tmp_path: Path, monkeypatch):
    d = tmp_path / "src"
    d.mkdir()
    (d / "a.py").write_text("x = 1\n", encoding="utf-8")
    (d / "b.py").write_text("y = 2\n", encoding="utf-8")

    progress_stream = TtyStringIO()
    original_get_text_stream = click.get_text_stream

    def get_text_stream(name: str):
        if name == "stderr":
            return progress_stream
        return original_get_text_stream(name)

    monkeypatch.setattr(click, "get_text_stream", get_text_stream)

    runner = CliRunner()
    result = runner.invoke(cli, ["evaluate", str(d), "-r", "--json"])

    assert result.exit_code == 0
    assert "results" in result.output
    assert progress_stream.getvalue() == ""


def test_evaluate_interrupt_exits_cleanly(tmp_path: Path, monkeypatch):
    f = tmp_path / "test.py"
    f.write_text("x = 1\n", encoding="utf-8")

    def interrupt(*args, **kwargs):
        raise KeyboardInterrupt

    monkeypatch.setattr("topos.cli.evaluation.run_classify_file", interrupt)

    runner = CliRunner()
    result = runner.invoke(cli, ["evaluate", str(f)])

    assert result.exit_code == 130
    assert "Interrupted. Exiting." in result.output


def test_compare_files(tmp_path: Path):
    f1 = tmp_path / "f1.py"
    f1.write_text("x = 1\n", encoding="utf-8")
    f2 = tmp_path / "f2.py"
    f2.write_text("x = 2\n", encoding="utf-8")

    runner = CliRunner()
    result = runner.invoke(cli, ["compare", str(f1), str(f2)])
    assert result.exit_code == 0
    assert "Edit Distance:" in result.output
    assert "Similarity:" in result.output


def test_inspect_file(tmp_path: Path):
    f = tmp_path / "test.py"
    f.write_text("def foo(): pass\n", encoding="utf-8")

    runner = CliRunner()
    result = runner.invoke(cli, ["inspect", str(f)])
    assert result.exit_code == 0
    assert "Classification" in result.output
    assert "Raw Metrics" in result.output
    assert "Entropy Analysis" in result.output


def _strip_ansi(text: str) -> str:
    return re.sub(r"\x1b\[[0-9;]*m", "", text)


def test_inspect_shows_security_findings_and_suggestions(tmp_path: Path):
    f = tmp_path / "danger.py"
    f.write_text("def f(x):\n    return eval(x)\n", encoding="utf-8")

    runner = CliRunner()
    result = runner.invoke(cli, ["inspect", str(f)])
    assert result.exit_code == 0
    out = _strip_ansi(result.output)
    assert "Security Findings" in out
    assert "Line 2" in out
    assert "eval" in out
    assert "Suggestions" in out


def test_inspect_go_file_uses_go_cpg_for_security_findings(tmp_path: Path):
    """`_build_cpg` must detect the file's language from its suffix, not
    default to Python — otherwise a non-Python CPG build silently finds
    nothing and every dangerous call goes unreported."""
    f = tmp_path / "danger.go"
    f.write_text(
        'package main\n\nimport "os/exec"\n\n'
        "func run(cmd string) {\n\texec.Command(cmd).Run()\n}\n",
        encoding="utf-8",
    )

    runner = CliRunner()
    result = runner.invoke(cli, ["inspect", str(f)])
    assert result.exit_code == 0
    out = _strip_ansi(result.output)
    assert "Security Findings" in out
    assert "exec.Command" in out


def test_inspect_allowlist_flips_verdict_and_caps_grade(tmp_path: Path):
    f = tmp_path / "conf.py"
    f.write_text("import yaml\ndef g(p):\n    return yaml.load(p)\n", encoding="utf-8")
    (tmp_path / ".topos.toml").write_text(
        '[[secure.allow]]\npattern = "yaml.load"\nreason = "trusted ML config"\n',
        encoding="utf-8",
    )

    runner = CliRunner()
    result = runner.invoke(cli, ["inspect", str(f)])
    assert result.exit_code == 0
    out = _strip_ansi(result.output)
    assert "FAIL (raw)" in out
    assert "PASS (acknowledged)" in out
    assert "trusted ML config" in out


def test_inspect_json_carries_findings_and_verdict(tmp_path: Path):
    f = tmp_path / "danger.py"
    f.write_text("def f(x):\n    return eval(x)\n", encoding="utf-8")

    runner = CliRunner()
    result = runner.invoke(cli, ["inspect", str(f), "--allow", "eval", "--json"])
    assert result.exit_code == 0
    data = json.loads(result.output)
    assert data["secure_raw"] is False
    assert data["secure_adjusted"] is True
    assert data["acknowledged_risks"][0]["callee"] == "eval"
    assert "suggestions" in data


def test_evaluate_json_carries_diagnostics(tmp_path: Path):
    f = tmp_path / "danger.py"
    f.write_text("def f(x):\n    return eval(x)\n", encoding="utf-8")

    runner = CliRunner()
    result = runner.invoke(cli, ["evaluate", str(f), "--json"])
    assert result.exit_code == 0
    data = json.loads(result.output)
    row = data["results"][0]
    assert row["security_findings"][0]["callee"] == "eval"
    assert row["secure_raw"] is False
    assert "suggestions" in row


def test_evaluate_verbose_lists_findings(tmp_path: Path):
    f = tmp_path / "danger.py"
    f.write_text("def f(x):\n    return eval(x)\n", encoding="utf-8")

    runner = CliRunner()
    result = runner.invoke(cli, ["evaluate", str(f), "--verbose"])
    assert result.exit_code == 0
    out = _strip_ansi(result.output)
    assert "Security Findings" in out
    assert "eval" in out


def test_priority_choice_matches_priority_enum():
    """quality.py's module-level ``_PRIORITY_VALUES`` literal (kept import-free
    to avoid eagerly loading the eval stack / numpy at CLI-registration time)
    must not drift from ``Priority``, the enum it mirrors."""
    from topos.cli.commands.quality import _PRIORITY_CHOICE, _PRIORITY_VALUES
    from topos.evaluation.policies.base import Priority

    assert set(_PRIORITY_VALUES) == {p.value for p in Priority}
    assert set(_PRIORITY_CHOICE.choices) == {p.value for p in Priority}
