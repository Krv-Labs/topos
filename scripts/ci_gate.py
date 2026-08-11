#!/usr/bin/env python3
"""Decide whether CI should run for the current event.

`ci.yml` used to restrict `pull_request` to `main`, the migration branch, and
`release/**`. A stacked PR targets its parent topic branch instead, so it
matched nothing and got no CI at all — the review window for a stacked change
had no verification behind it.

The trigger is therefore unfiltered and this script re-applies the policy:

* `push` — the trigger's own `branches` list already filtered it. Run.
* `pull_request` into a branch on `TRUNK_PATTERNS`. Run. (Unchanged behavior;
  decided before any API call, so trunk PRs never depend on the stack query.)
* `pull_request` that is part of a GitHub stack, whatever it targets. Run.
* Any other `pull_request` — a one-off PR aimed at someone's topic branch.
  Skip. This is the case the unfiltered trigger would otherwise let in.

Stack membership comes from the `PullRequestStack` GraphQL API, queried by the
workflow and handed to us as a file so this stays pure and testable. If that
response is missing, malformed, or carries an `errors` array, we **fail open**
and run: a permissions or schema problem must not silently skip verification.
That path is loud on purpose — see `--selftest` and the `::warning::` below.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import sys
from pathlib import Path

# Mirrors the `on.pull_request.branches` allowlist as it stood before that
# filter was removed. This list is now the only thing restricting which
# non-stacked PRs get CI, and nothing enforces the correspondence, so keep it in
# sync by hand.
#
# Note this is *not* a union with `on.push.branches`, which is deliberately
# narrower (no `release/**`). Pushes never consult this list: the trigger has
# already filtered them by the time the gate runs, so `decide()` admits any
# non-PR event outright.
TRUNK_PATTERNS = ("main", "worktree-rust-migration-v0.4.0", "release/**")


def matches_branch(ref: str, pattern: str) -> bool:
    """Match a ref against one GitHub Actions branch pattern.

    Actions globs are not `fnmatch`: `*` stops at `/` while `**` crosses it,
    so `release/*` must not match `release/a/b` but `release/**` must. Plain
    `fnmatch` maps both to `.*` and would quietly over-match.
    """
    out: list[str] = []
    i = 0
    while i < len(pattern):
        if pattern.startswith("**", i):
            out.append(".*")
            i += 2
        elif pattern[i] == "*":
            out.append("[^/]*")
            i += 1
        else:
            out.append(re.escape(pattern[i]))
            i += 1
    return re.fullmatch("".join(out), ref) is not None


def is_trunk(ref: str) -> bool:
    return any(matches_branch(ref, p) for p in TRUNK_PATTERNS)


def stack_number(response: str) -> int | None:
    """Pull the stack number out of a raw GraphQL response.

    Raises on anything unexpected so the caller can fail open rather than read
    a malformed payload as "not stacked".
    """
    payload = json.loads(response)
    if payload.get("errors"):
        raise ValueError(f"GraphQL errors: {payload['errors']}")
    pr = payload["data"]["repository"]["pullRequest"]
    stack = pr.get("stack")
    if stack is None:
        return None
    return int(stack["number"])


def decide(event_name: str, base_ref: str, response: str | None) -> tuple[bool, str]:
    """Return ``(run, reason)``. Never raises; unexpected input fails open."""
    if event_name != "pull_request":
        return True, f"event is {event_name!r}, not a pull request"
    if is_trunk(base_ref):
        return True, f"PR targets trunk branch {base_ref!r}"
    if not response or not response.strip():
        return True, (
            "::warning::FAIL-OPEN: no stack response available, so stack "
            "membership is unknown and CI is running anyway. The "
            "stacked-PR-only policy is NOT in effect for this run."
        )
    try:
        number = stack_number(response)
    except (ValueError, KeyError, TypeError, json.JSONDecodeError) as exc:
        return True, (
            f"::warning::FAIL-OPEN: could not read stack membership ({exc}), "
            "so CI is running anyway. The stacked-PR-only policy is NOT in "
            "effect for this run."
        )
    if number is None:
        return False, (
            f"PR targets {base_ref!r}, which is neither a trunk branch nor "
            "part of a stack"
        )
    return True, f"PR is part of stack #{number}"


SELFTEST_CASES: tuple[tuple[str, str, str | None, bool], ...] = (
    # Pushes are pre-filtered by the trigger and must never be gated off; this
    # is the one path that can break the default branch.
    ("push", "main", None, True),
    ("workflow_dispatch", "", None, True),
    # Trunk PRs keep the old behavior with no dependency on the stack API.
    ("pull_request", "main", None, True),
    ("pull_request", "release/v0.4.4", None, True),
    ("pull_request", "release/a/b", None, True),
    ("pull_request", "worktree-rust-migration-v0.4.0", None, True),
    # A stacked PR into a topic branch: the case that had no CI before.
    (
        "pull_request",
        "chore/rmcp-3x-264",
        '{"data":{"repository":{"pullRequest":{"stack":{"number":338}}}}}',
        True,
    ),
    # A one-off PR into a topic branch: the case we intend to skip.
    (
        "pull_request",
        "someones/topic",
        '{"data":{"repository":{"pullRequest":{"stack":null}}}}',
        False,
    ),
    # Fail-open paths: absent, malformed, and error responses all run CI.
    ("pull_request", "someones/topic", None, True),
    ("pull_request", "someones/topic", "not json", True),
    (
        "pull_request",
        "someones/topic",
        '{"errors":[{"message":"Field \'stack\' doesn\'t exist"}]}',
        True,
    ),
    ("pull_request", "someones/topic", '{"data":{"repository":null}}', True),
)


def selftest() -> int:
    failures = 0
    for event_name, base_ref, response, want in SELFTEST_CASES:
        got, reason = decide(event_name, base_ref, response)
        status = "ok" if got == want else "FAIL"
        if got != want:
            failures += 1
        print(f"  [{status}] {event_name} -> {base_ref!r}: run={got} ({reason})")
    # `release/*` must not cross a slash, or the mirror of the Actions filter
    # would be wrong in the permissive direction.
    if matches_branch("release/a/b", "release/*"):
        print("  [FAIL] 'release/*' matched across a slash")
        failures += 1
    if failures:
        print(f"selftest FAILED ({failures} case(s))", file=sys.stderr)
        return 1
    print(f"selftest passed ({len(SELFTEST_CASES)} cases)")
    return 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--event-name", default=os.environ.get("GITHUB_EVENT_NAME", ""))
    parser.add_argument("--base-ref", default="")
    parser.add_argument(
        "--stack-response",
        type=Path,
        help="File holding the raw GraphQL response for this PR's stack.",
    )
    parser.add_argument(
        "--selftest",
        action="store_true",
        help="Check the decision table without touching the network.",
    )
    args = parser.parse_args(argv)

    if args.selftest:
        return selftest()

    response: str | None = None
    if args.stack_response and args.stack_response.is_file():
        response = args.stack_response.read_text(encoding="utf-8")

    run, reason = decide(args.event_name, args.base_ref, response)
    print(f"CI gate: run={str(run).lower()} — {reason}")

    output = os.environ.get("GITHUB_OUTPUT")
    if output:
        with open(output, "a", encoding="utf-8") as handle:
            handle.write(f"run={str(run).lower()}\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
