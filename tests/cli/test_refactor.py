from __future__ import annotations

import json
from pathlib import Path

from click.testing import CliRunner
from topos.cli.main import cli


def test_refactor_help_lists_subcommands():
    result = CliRunner().invoke(cli, ["refactor", "--help"])
    assert result.exit_code == 0
    assert "cycles" in result.output
    assert "dependencies" in result.output
    assert "process" in result.output


def test_refactor_cycles_text_output(tmp_path: Path):
    source = (
        "def f(items):\n"
        "    total = 0\n"
        "    for x in items:\n"
        "        total += x\n"
        "    return total\n"
    )
    f = tmp_path / "loopy.py"
    f.write_text(source, encoding="utf-8")

    result = CliRunner().invoke(cli, ["refactor", "cycles", str(f)])
    assert result.exit_code == 0
    assert "betti_1=1" in result.output
    assert "cycle" in result.output


def test_refactor_cycles_json_output(tmp_path: Path):
    source = (
        "def f(items):\n"
        "    total = 0\n"
        "    for x in items:\n"
        "        total += x\n"
        "    return total\n"
    )
    f = tmp_path / "loopy.py"
    f.write_text(source, encoding="utf-8")

    result = CliRunner().invoke(cli, ["refactor", "cycles", str(f), "--json"])
    assert result.exit_code == 0
    payload = json.loads(result.output)
    assert isinstance(payload, list)
    assert payload[0]["kind"] == "cycle"


def test_refactor_cycles_no_loop_reports_no_hotspots(tmp_path: Path):
    f = tmp_path / "flat.py"
    f.write_text("def f():\n    return 1\n", encoding="utf-8")

    result = CliRunner().invoke(cli, ["refactor", "cycles", str(f)])
    assert result.exit_code == 0
    assert "betti_1=0" in result.output
    assert "none found" in result.output


def test_refactor_dependencies_without_gitnexus_errors_cleanly(
    tmp_path: Path, monkeypatch
):
    monkeypatch.chdir(tmp_path)
    f = tmp_path / "a.py"
    f.write_text("x = 1\n", encoding="utf-8")

    result = CliRunner().invoke(cli, ["refactor", "dependencies", str(f)])
    assert result.exit_code != 0
    assert "gitnexus" in result.output.lower()


def test_refactor_process_without_gitnexus_errors_cleanly(tmp_path: Path, monkeypatch):
    monkeypatch.chdir(tmp_path)
    f = tmp_path / "a.py"
    f.write_text("x = 1\n", encoding="utf-8")

    result = CliRunner().invoke(cli, ["refactor", "process", str(f)])
    assert result.exit_code != 0
    assert "gitnexus" in result.output.lower()
