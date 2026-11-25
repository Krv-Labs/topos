#!/usr/bin/env python3
"""Render packaging/homebrew/topos.rb.template into a Homebrew formula.

The Krv-Labs/homebrew-tap formula is this template with the release version
and the three binaries' sha256 values filled in. Rendering it in CI and
running `brew style` on the result catches a formula the tap would reject
before a release ships, not after: Homebrew adds style cops on its own
schedule (Homebrew/SingleUseMethod failed the v0.16.0 tap PR).

    # The tap formula for a published release, from its checksums.txt:
    render_homebrew_formula.py --version 0.16.0 --checksums checksums.txt -o topos.rb

    # A formula for style checks only (placeholder hashes):
    render_homebrew_formula.py --placeholder -o topos.rb
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
TEMPLATE = REPO_ROOT / "packaging" / "homebrew" / "topos.rb.template"

# Placeholder -> release asset whose sha256 fills it.
ASSETS = {
    "SHA_MACOS_ARM64": "topos-macos-arm64",
    "SHA_LINUX_AMD64": "topos-linux-amd64",
    "SHA_LINUX_ARM64": "topos-linux-arm64",
}
SHA256 = re.compile(r"^[0-9a-f]{64}$")
VERSION = re.compile(r"^\d+\.\d+\.\d+$")


def parse_checksums(text: str) -> dict[str, str]:
    """Map asset name to sha256 from `sha256sum`-style lines."""
    sums: dict[str, str] = {}
    for line in text.splitlines():
        parts = line.split()
        if len(parts) == 2 and SHA256.match(parts[0]):
            sums[parts[1].lstrip("*")] = parts[0]
    return sums


def render(template: str, version: str, shas: dict[str, str]) -> str:
    values = {"VERSION": version, **shas}
    out = template
    for key, value in values.items():
        out = out.replace("{{" + key + "}}", value)
    left = sorted(set(re.findall(r"\{\{(\w+)\}\}", out)))
    if left:
        raise ValueError(f"unfilled placeholders: {', '.join(left)}")
    return out


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    source = parser.add_mutually_exclusive_group(required=True)
    source.add_argument("--checksums", type=Path, help="the release's checksums.txt")
    source.add_argument(
        "--placeholder",
        action="store_true",
        help="fill version 0.0.0 and dummy hashes, for style checks",
    )
    parser.add_argument("--version", help="release version, e.g. 0.16.0 (no leading v)")
    parser.add_argument(
        "--template", type=Path, default=TEMPLATE, help=argparse.SUPPRESS
    )
    parser.add_argument("-o", "--output", type=Path, required=True)
    args = parser.parse_args(argv)

    if args.placeholder:
        version = "0.0.0"
        shas = {key: "0" * 64 for key in ASSETS}
    else:
        if not args.version or not VERSION.match(args.version):
            parser.error("--version X.Y.Z is required with --checksums")
        version = args.version
        sums = parse_checksums(args.checksums.read_text(encoding="utf-8"))
        missing = [asset for asset in ASSETS.values() if asset not in sums]
        if missing:
            parser.error(f"checksums.txt has no entry for: {', '.join(missing)}")
        shas = {key: sums[asset] for key, asset in ASSETS.items()}

    try:
        formula = render(args.template.read_text(encoding="utf-8"), version, shas)
    except ValueError as err:
        print(f"error: {err}", file=sys.stderr)
        return 1
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(formula, encoding="utf-8")
    return 0


if __name__ == "__main__":
    sys.exit(main())
