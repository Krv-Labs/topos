from __future__ import annotations

import io
from pathlib import Path

import click
import topos.cli.commands.quality as quality_commands
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
    assert f"{f} [" in result.output
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

    monkeypatch.setattr(quality_commands, "run_classify_file", interrupt)

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
