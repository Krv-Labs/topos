#!/usr/bin/env python3
"""Fail if any published version string diverges from Cargo.toml."""

from __future__ import annotations

import json
import runpy
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def cargo_version() -> str:
    with (ROOT / "Cargo.toml").open("rb") as f:
        return tomllib.load(f)["workspace"]["package"]["version"]


def python_source_version() -> str:
    version_globals = runpy.run_path(str(ROOT / "topos/_version.py"))
    return version_globals["_cargo_version"]()


def main() -> int:
    expected = cargo_version()
    errors: list[str] = []

    package_json = json.loads(
        (ROOT / "extensions/vscode/package.json").read_text(encoding="utf-8")
    )
    if package_json["version"] != expected:
        errors.append(
            "extensions/vscode/package.json "
            f"has {package_json['version']!r}, expected {expected!r}"
        )

    mcp_manifest = json.loads((ROOT / ".mcp/server.json").read_text(encoding="utf-8"))
    if mcp_manifest["version"] != expected:
        errors.append(
            f".mcp/server.json has {mcp_manifest['version']!r}, expected {expected!r}"
        )

    for package in mcp_manifest["packages"]:
        if package["version"] != expected:
            errors.append(
                ".mcp/server.json package "
                f"{package['identifier']!r} has {package['version']!r}, "
                f"expected {expected!r}"
            )

    python_version = python_source_version()
    if python_version != expected:
        errors.append(
            f"topos._version._cargo_version() is {python_version!r}, "
            f"expected {expected!r}"
        )

    if errors:
        for message in errors:
            print(message, file=sys.stderr)
        return 1

    print(f"version check passed ({expected})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
