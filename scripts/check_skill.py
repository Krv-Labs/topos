#!/usr/bin/env python3
"""Validate bundled agent skill folders under skills/."""

from __future__ import annotations

import re
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SKILLS_ROOT = ROOT / "skills"
SLUG_RE = re.compile(r"^[a-z0-9][a-z0-9-]*$")
FRONTMATTER_RE = re.compile(r"^---\s*\n(.*?)\n---\s*\n", re.DOTALL)
REQUIRED_SECTIONS = ("## When to Use", "## Pitfalls", "## Verification")
REQUIRED_FRONTMATTER_KEYS = ("name:", "description:", "version:")


def cargo_version() -> str:
    with (ROOT / "Cargo.toml").open("rb") as f:
        return tomllib.load(f)["package"]["version"]


def parse_scalar(block: str, key: str) -> str | None:
    match = re.search(rf"^{re.escape(key)}:\s*(.+)$", block, re.MULTILINE)
    if not match:
        return None
    value = match.group(1).strip()
    if value.startswith('"') and value.endswith('"'):
        return value[1:-1]
    if value.startswith("'") and value.endswith("'"):
        return value[1:-1]
    return value


def frontmatter_contains(block: str, needle: str) -> bool:
    return needle in block


def validate_skill(skill_dir: Path, expected_version: str) -> list[str]:
    errors: list[str] = []
    skill_md = skill_dir / "SKILL.md"
    if not skill_md.is_file():
        return [f"{skill_dir}: missing SKILL.md"]

    text = skill_md.read_text(encoding="utf-8")
    match = FRONTMATTER_RE.match(text)
    if not match:
        return [f"{skill_md}: missing YAML frontmatter delimiters"]

    frontmatter = match.group(1)
    body = text[match.end() :]

    for key in REQUIRED_FRONTMATTER_KEYS:
        if not frontmatter_contains(frontmatter, key):
            errors.append(f"{skill_md}: frontmatter missing {key.rstrip(':')}")

    name = parse_scalar(frontmatter, "name")
    if not name:
        errors.append(f"{skill_md}: frontmatter name is required")
    elif name != skill_dir.name:
        errors.append(
            f"{skill_md}: frontmatter name {name!r} must match directory {skill_dir.name!r}"
        )
    elif not SLUG_RE.fullmatch(name):
        errors.append(f"{skill_md}: name {name!r} must match {SLUG_RE.pattern}")

    description = parse_scalar(frontmatter, "description")
    if not description:
        errors.append(f"{skill_md}: frontmatter description is required")
    elif len(description) > 160:
        errors.append(
            f"{skill_md}: description is {len(description)} chars; keep under 160"
        )

    version = parse_scalar(frontmatter, "version")
    if not version:
        errors.append(f"{skill_md}: frontmatter version is required")
    elif version != expected_version:
        errors.append(
            f"{skill_md}: version {version!r} must match Cargo.toml {expected_version!r}"
        )

    for section in REQUIRED_SECTIONS:
        if section not in body:
            errors.append(f"{skill_md}: missing section {section}")

    if "metadata:" not in frontmatter:
        errors.append(f"{skill_md}: frontmatter missing metadata block")
    else:
        if "openclaw:" not in frontmatter:
            errors.append(f"{skill_md}: metadata.openclaw block is required")
        elif "bins:" not in frontmatter or "topos" not in frontmatter:
            errors.append(f"{skill_md}: metadata.openclaw.requires.bins must include topos")

        if "hermes:" not in frontmatter:
            errors.append(f"{skill_md}: metadata.hermes block is required")
        else:
            if "tags:" not in frontmatter:
                errors.append(f"{skill_md}: metadata.hermes.tags is required")
            if "category:" not in frontmatter:
                errors.append(f"{skill_md}: metadata.hermes.category is required")

    return errors


def main() -> int:
    if not SKILLS_ROOT.is_dir():
        print(f"skills root not found: {SKILLS_ROOT}", file=sys.stderr)
        return 1

    skill_dirs = sorted(
        path
        for path in SKILLS_ROOT.iterdir()
        if path.is_dir() and not path.name.startswith(".")
    )
    if not skill_dirs:
        print(f"no skill folders found under {SKILLS_ROOT}", file=sys.stderr)
        return 1

    expected_version = cargo_version()
    errors: list[str] = []
    for skill_dir in skill_dirs:
        errors.extend(validate_skill(skill_dir, expected_version))

    if errors:
        for message in errors:
            print(message, file=sys.stderr)
        return 1

    print(f"skill check passed ({len(skill_dirs)} skill(s), version {expected_version})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
