#!/usr/bin/env python3
"""Fail if any published version string diverges from Cargo.toml."""

from __future__ import annotations

import argparse
import json
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def cargo_version() -> str:
    with (ROOT / "Cargo.toml").open("rb") as f:
        return tomllib.load(f)["workspace"]["package"]["version"]


def normalize_tag(tag: str) -> str:
    """Strip a leading ``v`` so ``v0.4.4`` and ``0.4.4`` compare equal."""
    return tag[1:] if tag.startswith("v") else tag


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--tag",
        help=(
            "Release git tag (with or without leading v). When set, must equal "
            "the Cargo.toml workspace version so GitHub/Homebrew tags cannot "
            "drift from the maturin/PyPI wheel version."
        ),
    )
    args = parser.parse_args(argv)

    expected = cargo_version()
    errors: list[str] = []

    if args.tag is not None:
        tag_version = normalize_tag(args.tag)
        if tag_version != expected:
            errors.append(
                f"release tag {args.tag!r} normalizes to {tag_version!r}, "
                f"expected Cargo.toml workspace version {expected!r}"
            )

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
        # ponytail: pypi-only guard; generalize if a second registryType lands
        if package.get("registryType") == "pypi":
            if "registryBaseUrl" in package:
                errors.append(
                    ".mcp/server.json pypi package must omit registryBaseUrl: "
                    "VS Code appends --index-url <registryBaseUrl> unconditionally, "
                    "and the only publishable value is not a PEP 503 index"
                )
            if any(
                arg.get("name") == "--index-url"
                for arg in package.get("runtimeArguments", [])
            ):
                errors.append(
                    ".mcp/server.json pypi package must not pin --index-url: "
                    "uv rejects the flag when VS Code duplicates it"
                )

    if errors:
        for message in errors:
            print(message, file=sys.stderr)
        return 1

    if args.tag is not None:
        print(f"version check passed ({expected}; tag {args.tag})")
    else:
        print(f"version check passed ({expected})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
