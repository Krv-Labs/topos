"""Tests for version resolution."""

from __future__ import annotations

from importlib.metadata import PackageNotFoundError
from unittest.mock import patch

import topos._version as version_mod


def test_get_version_uses_installed_metadata():
    with patch("topos._version.version", return_value="1.2.3"):
        assert version_mod.get_version() == "1.2.3"


def test_get_version_falls_back_to_cargo_toml():
    with (
        patch("topos._version.version", side_effect=PackageNotFoundError()),
        patch("topos._version._cargo_version", return_value="0.4.0"),
    ):
        assert version_mod.get_version() == "0.4.0"


def test_cargo_version_matches_repo():
    import tomllib
    from pathlib import Path

    cargo_path = Path(__file__).resolve().parent.parent / "Cargo.toml"
    with cargo_path.open("rb") as f:
        expected = tomllib.load(f)["package"]["version"]
    assert version_mod._cargo_version() == expected
