"""Dogfood tests for PyInstaller release binaries (Issue #110).

Run against a built onefile binary via ``TOPOS_BINARY``. CI builds the
binary, then asserts startup budgets, version metadata, help output, and
core subcommand wiring before artifacts ship.
"""

from __future__ import annotations

import os

import pytest
from tests.packaging.conftest import expected_cargo_version, run_topos_binary

pytestmark = pytest.mark.skipif(
    not os.environ.get("TOPOS_BINARY"),
    reason="Set TOPOS_BINARY to dogfood a PyInstaller binary",
)

_CORE_COMMANDS = ("evaluate", "coverage", "mcp", "compare", "inspect", "depgraph")


def test_frozen_binary_version_matches_cargo_toml() -> None:
    """``topos --version`` must report Cargo.toml version, not the fallback."""
    expected = expected_cargo_version()
    result = run_topos_binary(["--version"])
    assert result.returncode == 0, result.stderr
    assert f"topos, version {expected}" in result.stdout
    assert "0.0.0+unknown" not in result.stdout


def test_frozen_binary_root_help_lists_commands() -> None:
    result = run_topos_binary(["--help"])
    assert result.returncode == 0, result.stderr
    for command in _CORE_COMMANDS:
        assert command in result.stdout, f"missing {command!r} in --help"


def test_frozen_binary_evaluate_help() -> None:
    result = run_topos_binary(["evaluate", "--help"])
    assert result.returncode == 0, result.stderr
    assert "evaluate" in result.stdout.lower()


def test_frozen_binary_coverage_help() -> None:
    result = run_topos_binary(["coverage", "--help"])
    assert result.returncode == 0, result.stderr
    assert "coverage" in result.stdout.lower()


def test_frozen_binary_mcp_help() -> None:
    result = run_topos_binary(["mcp", "--help"])
    assert result.returncode == 0, result.stderr
