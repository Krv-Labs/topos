#!/usr/bin/env python3
"""Fail if a GitHub Actions workflow pushes commits straight to a branch.

main's ruleset takes changes only through a pull request, so a workflow step
that runs `git push` is rejected the first time it has something to commit.
The self-badge workflow did exactly that and failed on every badge change
after v0.15.0. Workflows that publish a change must open a pull request
(peter-evans/create-pull-request, as openwiki.yml and badge.yml do).

Pushing a tag (`git push origin vX.Y.Z`, `git push --tags`) is allowed: tags
are not branches and the ruleset does not cover them.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

WORKFLOWS = Path(__file__).resolve().parent.parent / ".github" / "workflows"
GIT_PUSH = re.compile(r"\bgit\s+push\b(?P<rest>[^\n#]*)")
TAG_PUSH = re.compile(r"--tags\b|\brefs/tags/|\bv\$\{|\bv\d")


def violations(text: str) -> list[tuple[int, str]]:
    found = []
    for number, line in enumerate(text.splitlines(), start=1):
        if line.lstrip().startswith("#"):
            continue
        match = GIT_PUSH.search(line)
        if match and not TAG_PUSH.search(match.group("rest")):
            found.append((number, line.strip()))
    return found


def main() -> int:
    failed = False
    for path in sorted(WORKFLOWS.glob("*.y*ml")):
        for number, line in violations(path.read_text(encoding="utf-8")):
            failed = True
            rel = path.relative_to(WORKFLOWS.parent.parent)
            print(
                f"::error file={rel},line={number}::{rel}:{number}: `{line}` pushes "
                "to a branch, but main only takes changes through a pull request. "
                "Open one with peter-evans/create-pull-request instead (see "
                "badge.yml or openwiki.yml)."
            )
    if not failed:
        print("No workflow pushes to a branch.")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
