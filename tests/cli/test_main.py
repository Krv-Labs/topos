from __future__ import annotations

from click.testing import CliRunner
from topos import __version__
from topos.cli.main import cli


def test_cli_version():
    runner = CliRunner()
    result = runner.invoke(cli, ["--version"])
    assert result.exit_code == 0
    assert f"topos, version {__version__}" in result.output


def test_cli_help():
    runner = CliRunner()
    result = runner.invoke(cli, ["--help"])
    assert result.exit_code == 0
    assert "Topos: Category-theoretic code quality evaluation." in result.output
    assert "evaluate" in result.output
    assert "compare" in result.output
    assert "inspect" in result.output
    assert "coverage" in result.output
    assert "uninstall" in result.output
    assert "mcp" in result.output
    assert "depgraph" in result.output
