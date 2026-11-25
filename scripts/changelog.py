#!/usr/bin/env python3
"""Changelog fragments: one file per PR, folded into CHANGELOG.md at release.

Every PR used to add its entry under `## [Unreleased]` in CHANGELOG.md. Parallel
PRs all edited the same lines, so each merge re-conflicted the rest: on
2026-11-24 ten PRs needed nine extra pushes and admin merges, and one hand
resolution misfiled main's entries. A PR now adds its own file instead:

    changes/<slug>.<kind>.md      kind: added | changed | deprecated |
                                  removed | fixed | security

holding one or more Keep a Changelog bullets (lines starting with `- `). No two
PRs touch the same file, so they never conflict.

    changelog.py check                 # CI: fragments are well formed and
                                       # [Unreleased] in CHANGELOG.md is empty
    changelog.py release 0.17.0        # release PR: fold fragments into
        [--date YYYY-MM-DD]            # `## [0.17.0] - <date>` and delete them
"""

from __future__ import annotations

import argparse
import datetime as dt
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CHANGELOG = ROOT / "CHANGELOG.md"
CHANGES = ROOT / "changes"

# Keep a Changelog headings, in the order a release section lists them.
KINDS = {
    "added": "Added",
    "changed": "Changed",
    "deprecated": "Deprecated",
    "removed": "Removed",
    "fixed": "Fixed",
    "security": "Security",
}
FRAGMENT = re.compile(r"^(?P<slug>[a-z0-9][a-z0-9._-]*)\.(?P<kind>[a-z]+)\.md$")
UNRELEASED = re.compile(r"^## \[Unreleased\][ \t]*\n", re.MULTILINE)
NEXT_SECTION = re.compile(r"^## \[", re.MULTILINE)
VERSION = re.compile(r"^\d+\.\d+\.\d+$")
FIX = (
    "Write the entry as a fragment file instead: changes/<slug>.<kind>.md, where "
    f"<kind> is one of {', '.join(KINDS)} and the file holds `- ` bullets. "
    "See CONTRIBUTING.md."
)


def fragment_files(changes: Path) -> list[Path]:
    if not changes.is_dir():
        return []
    return sorted(p for p in changes.iterdir() if p.is_file() and p.name != "README.md")


def fragment_errors(changes: Path) -> list[str]:
    errors = []
    for path in fragment_files(changes):
        rel = f"changes/{path.name}"
        match = FRAGMENT.match(path.name)
        if not match:
            errors.append(f"{rel}: name must be <slug>.<kind>.md. {FIX}")
            continue
        if match.group("kind") not in KINDS:
            errors.append(
                f"{rel}: unknown kind {match.group('kind')!r}; use one of "
                f"{', '.join(KINDS)}."
            )
        text = path.read_text(encoding="utf-8").strip()
        if not text.startswith("- "):
            errors.append(
                f"{rel}: must hold Keep a Changelog bullets, each starting with "
                "`- ` (continuation lines indented two spaces)."
            )
    return errors


def unreleased_body(changelog_text: str) -> str:
    match = UNRELEASED.search(changelog_text)
    if not match:
        raise ValueError("CHANGELOG.md has no `## [Unreleased]` heading")
    rest = changelog_text[match.end() :]
    following = NEXT_SECTION.search(rest)
    return rest[: following.start()] if following else rest


def check(changes: Path, changelog: Path) -> list[str]:
    errors = fragment_errors(changes)
    try:
        body = unreleased_body(changelog.read_text(encoding="utf-8"))
    except ValueError as err:
        return errors + [str(err)]
    if body.strip():
        errors.append(
            "CHANGELOG.md: `## [Unreleased]` has entries. Parallel PRs editing "
            f"that section conflict on every merge. {FIX}"
        )
    return errors


def landed_at(path: Path) -> int:
    """When the fragment was added to git, so entries list in merge order."""
    try:
        out = subprocess.run(
            ["git", "log", "--diff-filter=A", "--format=%ct", "--", str(path)],
            cwd=path.parent,
            capture_output=True,
            text=True,
            check=True,
        ).stdout.split()
    except (OSError, subprocess.CalledProcessError):
        return sys.maxsize
    return int(out[-1]) if out else sys.maxsize


def release_section(changes: Path, version: str, date: str) -> tuple[str, list[Path]]:
    by_kind: dict[str, list[str]] = {kind: [] for kind in KINDS}
    files = fragment_files(changes)
    for path in sorted(files, key=lambda p: (landed_at(p), p.name)):
        kind = FRAGMENT.match(path.name).group("kind")
        by_kind[kind].append(path.read_text(encoding="utf-8").strip())
    blocks = [
        f"### {KINDS[kind]}\n\n" + "\n".join(entries)
        for kind, entries in by_kind.items()
        if entries
    ]
    if not blocks:
        raise ValueError("no fragments in changes/: nothing to release")
    return f"## [{version}] - {date}\n\n" + "\n\n".join(blocks) + "\n\n", files


def release(changes: Path, changelog: Path, version: str, date: str) -> list[Path]:
    errors = check(changes, changelog)
    if errors:
        raise ValueError("\n".join(errors))
    text = changelog.read_text(encoding="utf-8")
    if f"## [{version}]" in text:
        raise ValueError(f"CHANGELOG.md already has a `## [{version}]` section")
    section, files = release_section(changes, version, date)
    match = UNRELEASED.search(text)
    rest = text[match.end() :].lstrip("\n")
    changelog.write_text(text[: match.end()] + "\n" + section + rest, encoding="utf-8")
    for path in files:
        path.unlink()
    return files


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    sub = parser.add_subparsers(dest="command", required=True)
    sub.add_parser("check", help="validate fragments and an empty [Unreleased]")
    rel = sub.add_parser("release", help="fold fragments into a version section")
    rel.add_argument("version")
    rel.add_argument("--date", default=dt.date.today().isoformat())
    parser.add_argument("--changes", type=Path, default=CHANGES, help=argparse.SUPPRESS)
    parser.add_argument(
        "--changelog", type=Path, default=CHANGELOG, help=argparse.SUPPRESS
    )
    args = parser.parse_args(argv)

    if args.command == "check":
        errors = check(args.changes, args.changelog)
        for error in errors:
            print(f"::error title=Changelog::{error}")
        if not errors:
            print(f"{len(fragment_files(args.changes))} changelog fragment(s) OK.")
        return 1 if errors else 0

    if not VERSION.match(args.version):
        parser.error("version must be X.Y.Z, with no leading v")
    try:
        folded = release(args.changes, args.changelog, args.version, args.date)
    except ValueError as err:
        print(f"error: {err}", file=sys.stderr)
        return 1
    print(f"Folded {len(folded)} fragment(s) into ## [{args.version}] - {args.date}.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
