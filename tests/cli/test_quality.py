from __future__ import annotations

from pathlib import Path

from click.testing import CliRunner
from topos.cli.main import cli


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
    assert "Overall:" in result.output


def test_evaluate_recursive(tmp_path: Path):
    d = tmp_path / "src"
    d.mkdir()
    (d / "a.py").write_text("x = 1\n", encoding="utf-8")
    (d / "b.py").write_text("y = 2\n", encoding="utf-8")

    runner = CliRunner()
    result = runner.invoke(cli, ["evaluate", str(d), "-r"])
    assert result.exit_code == 0
    assert "a.py" in result.output
    assert "b.py" in result.output


def test_evaluate_json(tmp_path: Path):
    f = tmp_path / "test.py"
    f.write_text("x = 1\n", encoding="utf-8")

    runner = CliRunner()
    result = runner.invoke(cli, ["evaluate", str(f), "--json"])
    assert result.exit_code == 0
    import json

    # The output might contain additional text after the JSON.
    # Find the end of the JSON string (which should be the outermost brace).
    output = result.output
    try:
        # Assuming the JSON object is at the beginning.
        # find the end of the JSON object by counting braces or simply taking the
        # string until the matching '}'
        # A simpler way since it outputs a dict:
        json_str = output[: output.rfind("}") + 1]
        data = json.loads(json_str)
    except json.JSONDecodeError:
        # Fallback if the above heuristic fails
        data = json.loads(output.split("\n\n")[0])

    assert "results" in data
    assert data["results"][0]["file"] == str(f)


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
