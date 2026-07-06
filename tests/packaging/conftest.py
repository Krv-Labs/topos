"""Shared helpers for PyInstaller binary dogfood tests."""

from __future__ import annotations

import os
import subprocess
import tomllib
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]


def expected_cargo_version() -> str:
    """Version string from Cargo.toml — canonical release version."""
    with (REPO_ROOT / "Cargo.toml").open("rb") as f:
        return tomllib.load(f)["package"]["version"]


def run_topos_binary(
    args: list[str], *, timeout: float = 60.0
) -> subprocess.CompletedProcess[str]:
    binary = os.environ["TOPOS_BINARY"]
    return subprocess.run(
        [binary, *args],
        capture_output=True,
        text=True,
        timeout=timeout,
        check=False,
    )
