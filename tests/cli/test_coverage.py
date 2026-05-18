from __future__ import annotations

from pathlib import Path

from click.testing import CliRunner
from topos.cli.main import cli


def test_coverage_basic(tmp_path: Path):
    put = tmp_path / "put.py"
    put.write_text("def add(a, b): return a + b\n", encoding="utf-8")
    test = tmp_path / "test.py"
    test.write_text(
        "from put import add\ndef test_add(): assert add(1, 2) == 3\n", encoding="utf-8"
    )

    runner = CliRunner()
    result = runner.invoke(
        cli, ["structural-test-coverage", "--tests", str(test), str(put)]
    )
    assert result.exit_code == 0
    assert "Structural test coverage (declaration-level bipartite)" in result.output
    assert "Mean declaration coverage" in result.output
    assert "F2 score (beta=2)" in result.output


def test_coverage_threshold(tmp_path: Path):
    put = tmp_path / "put.py"
    put.write_text("def add(a, b): return a + b\n", encoding="utf-8")
    test = tmp_path / "test.py"
    test.write_text(
        "from put import add\ndef test_add(): assert add(1, 2) == 3\n", encoding="utf-8"
    )

    runner = CliRunner()
    result = runner.invoke(
        cli,
        [
            "structural-test-coverage",
            "--coverage-threshold",
            "0.2",
            "--tests",
            str(test),
            str(put),
        ],
    )
    assert result.exit_code == 0
    assert "Coverage threshold:         0.20" in result.output


def test_coverage_json(tmp_path: Path):
    put = tmp_path / "put.py"
    put.write_text("def add(a, b): return a + b\n", encoding="utf-8")
    test = tmp_path / "test.py"
    test.write_text(
        "from put import add\ndef test_add(): assert add(1, 2) == 3\n", encoding="utf-8"
    )

    runner = CliRunner()
    result = runner.invoke(
        cli, ["structural-test-coverage", "--json", "--tests", str(test), str(put)]
    )
    assert result.exit_code == 0
    import json

    data = json.loads(result.output)
    assert "mean_declaration_coverage" in data
    assert "f2_score" in data
    assert data["language"] == "python"


def test_coverage_invalid_k(tmp_path: Path):
    put = tmp_path / "put.py"
    put.write_text("def add(a, b): return a + b\n", encoding="utf-8")
    test = tmp_path / "test.py"
    test.write_text(
        "from put import add\ndef test_add(): assert add(1, 2) == 3\n", encoding="utf-8"
    )

    runner = CliRunner()
    result = runner.invoke(
        cli, ["structural-test-coverage", "--k", "0", "--tests", str(test), str(put)]
    )
    assert result.exit_code != 0
    assert "Error: --k must be >= 1." in result.output
