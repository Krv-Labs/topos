from __future__ import annotations

import json
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
    result = runner.invoke(cli, ["coverage", "--tests", str(test), str(put)])
    assert result.exit_code == 0
    assert "Topos Structural & Semantic Test Coverage" in result.output
    assert "Mean declaration coverage" in result.output
    assert "F2 score (beta=2)" in result.output
    assert "Topological CPG Semantic Coverage" in result.output
    assert "Topological coverage score" in result.output


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
            "coverage",
            "--coverage-threshold",
            "0.2",
            "--tests",
            str(test),
            str(put),
        ],
    )
    assert result.exit_code == 0
    assert "Coverage threshold:         0.20" in result.output
    assert "Topological threshold:      0.20" in result.output


def test_coverage_json(tmp_path: Path):
    put = tmp_path / "put.py"
    put.write_text("def add(a, b): return a + b\n", encoding="utf-8")
    test = tmp_path / "test.py"
    test.write_text(
        "from put import add\ndef test_add(): assert add(1, 2) == 3\n", encoding="utf-8"
    )

    runner = CliRunner()
    result = runner.invoke(cli, ["coverage", "--json", "--tests", str(test), str(put)])
    assert result.exit_code == 0

    data = json.loads(result.output)
    assert "mean_declaration_coverage" in data
    assert "f2_score" in data
    assert "topological_coverage" in data
    assert "coverage_score" in data["topological_coverage"]
    assert "distance" in data["topological_coverage"]
    assert "tested_functions" in data["topological_coverage"]
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
        cli, ["coverage", "--k", "0", "--tests", str(test), str(put)]
    )
    assert result.exit_code != 0
    assert "Error: --k must be >= 1." in result.output
