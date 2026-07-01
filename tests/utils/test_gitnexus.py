"""Tests for the shared GitNexus depgraph-generation helper."""

from __future__ import annotations

import subprocess
from pathlib import Path
from unittest.mock import patch

from topos.utils.gitnexus import (
    DEFAULT_ANALYZE_TIMEOUT_S,
    _resolve_timeout,
    generate_depgraph,
)


def test_missing_gitnexus_returns_structured_failure() -> None:
    with patch("topos.utils.gitnexus.gitnexus_available", return_value=False):
        result = generate_depgraph(Path("/tmp"))
    assert result.ok is False
    assert result.returncode == 127
    assert "npm install -g gitnexus" in result.message


def test_timeout_is_converted_to_structured_failure() -> None:
    with (
        patch("topos.utils.gitnexus.gitnexus_available", return_value=True),
        patch(
            "subprocess.run",
            side_effect=subprocess.TimeoutExpired(cmd="gitnexus", timeout=300.0),
        ),
    ):
        result = generate_depgraph(Path("/tmp"))
    assert result.ok is False
    assert result.returncode == 124
    assert "timed out" in result.message


def test_oserror_is_converted_to_structured_failure() -> None:
    with (
        patch("topos.utils.gitnexus.gitnexus_available", return_value=True),
        patch("subprocess.run", side_effect=OSError("permission denied")),
    ):
        result = generate_depgraph(Path("/tmp"))
    assert result.ok is False
    assert result.returncode == 126
    assert "could not be executed" in result.message


def test_default_timeout_passed_to_subprocess() -> None:
    with (
        patch("topos.utils.gitnexus.gitnexus_available", return_value=True),
        patch("subprocess.run") as mock_run,
    ):
        mock_run.return_value.returncode = 0
        mock_run.return_value.stdout = "ok"
        generate_depgraph(Path("/tmp"))
    assert mock_run.call_args.kwargs["timeout"] == DEFAULT_ANALYZE_TIMEOUT_S


def test_env_var_overrides_and_disables_timeout(monkeypatch) -> None:
    monkeypatch.setenv("TOPOS_DEPGRAPH_TIMEOUT", "42")
    assert _resolve_timeout(None) == 42.0
    monkeypatch.setenv("TOPOS_DEPGRAPH_TIMEOUT", "0")  # non-positive disables
    assert _resolve_timeout(None) is None
    monkeypatch.setenv("TOPOS_DEPGRAPH_TIMEOUT", "garbage")  # falls back
    assert _resolve_timeout(None) == DEFAULT_ANALYZE_TIMEOUT_S
    # An explicit argument wins over the env var.
    assert _resolve_timeout(10.0) == 10.0
