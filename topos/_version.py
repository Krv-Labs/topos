"""Package version.

Installed wheels/sdists get their version from maturin, which reads
``[package].version`` in ``Cargo.toml``. Editable checkouts fall back to
that same file so local dev matches the release source of truth.
"""

from __future__ import annotations

import tomllib
from importlib.metadata import PackageNotFoundError, version
from pathlib import Path

_PACKAGE = "topos-mcp"
_CARGO_TOML = Path(__file__).resolve().parent.parent / "Cargo.toml"


def _cargo_version() -> str:
    with _CARGO_TOML.open("rb") as f:
        return tomllib.load(f)["package"]["version"]


def get_version() -> str:
    try:
        return version(_PACKAGE)
    except PackageNotFoundError:
        return _cargo_version()


__version__ = get_version()
