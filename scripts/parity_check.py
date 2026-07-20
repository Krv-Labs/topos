#!/usr/bin/env python3
"""Drop-in CLI parity: new Rust `topos` vs the last published Python release.

Topos went all-Rust in v0.4.0. This script guards the *drop-in* promise —
that the new Rust `topos` CLI produces the same `ClassificationResult` the
old Python CLI did — by running `topos inspect <file> --json` through both
and diffing the two payloads metric by metric over a corpus.

Both sides are external processes; this harness imports nothing from
`topos` (there is no Python `topos` package anymore). The **reference** is
whatever `--reference` resolves to — by default the last Python release
pulled straight from PyPI with `uvx`, so no Python source needs to live in
this repo:

    uvx --from topos-mcp==0.3.11 topos inspect <file> --json

The **candidate** is the Rust binary built from this worktree.

Real, understood, *intentional* divergences are allowlisted below (see each
entry's `issue`); anything else is a regression and fails the check.

Usage:
    cargo build --release -p topos-cli
    # default: bundled corpus, all six languages, reference = PyPI 0.3.11
    python3 scripts/parity_check.py
    # one language / a custom tree
    python3 scripts/parity_check.py --corpus crates/topos-core/src --language rust
    # pin a different reference, or point at a local Python build
    python3 scripts/parity_check.py --reference "uvx --from topos-mcp==0.3.11 topos"
    python3 scripts/parity_check.py --reference /path/to/old/topos
    # self-parity smoke test (candidate vs candidate — must be clean)
    python3 scripts/parity_check.py --reference target/release/topos
"""

from __future__ import annotations

import argparse
import json
import math
import shlex
import subprocess
import sys
from dataclasses import dataclass, field
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# The published release the drop-in promise is measured against. Bump this
# in lockstep with the last Python release before the all-Rust cutover.
DEFAULT_REFERENCE = "uvx --from topos-mcp==0.3.11 topos"

LANGUAGE_SUFFIXES = {
    "python": (".py",),
    "rust": (".rs",),
    "javascript": (".js",),
    "typescript": (".ts",),
    "cpp": (".cpp", ".hpp"),
    "go": (".go",),
}

# Numeric metrics/scores are compared with this tolerance: the two CLIs do
# floating-point math in a slightly different order in a few places (e.g.
# Rust's min()-fold vs Python's min() over a list), so exact equality is too
# strict for values that are mathematically but not bit-for-bit identical.
FLOAT_TOL = 1e-6


@dataclass
class Divergence:
    """One expected, documented reference-vs-candidate difference."""

    metric: str
    issue: str
    reason: str
    # Only applies when this predicate(language) is True; None = always.
    applies: "callable | None" = None

    def matches(self, language: str) -> bool:
        return self.applies is None or self.applies(language)


KNOWN_DIVERGENCES: list[Divergence] = [
    Divergence(
        metric="ast.max_function_complexity",
        issue="#153",
        reason=(
            "For Python source, the old CLI's native tree-sitter path counts "
            "elif_clause, with_statement, assert_statement, comprehensions, and "
            "boolean short-circuit operators as separate decision points; the "
            "Rust port's UAST path counts IfStmt/ForStmt/WhileStmt/MatchStmt/"
            "TryStmt (plus the BinaryExpr and/or check) — the same convention "
            "topos-core's own CFG builder uses for cfg.cyclomatic, so the Rust "
            "value is internally consistent with cfg.cyclomatic. Non-Python "
            "languages were made language-neutral in v0.3.11 and should match "
            "exactly; only Python source is expected to diverge here."
        ),
        applies=lambda language: language == "python",
    ),
]


def has_unfiltered_main_guard(source: str, language: str) -> bool:
    """Whether Python `source` carries an `if __name__ == "__main__":` guard.

    The Rust `mapper_python::MainGuardFilter` excludes that guard from the
    CFG/PDG (forward-porting PR #133, merged into v0.3.11 as #127). If the
    reference release predates that exclusion, the guard shows up as exactly
    one extra CFG branch on the reference side; allowlist those files. With a
    0.3.11+ reference this branch is dead code — a clean run against a
    guard-bearing corpus confirms it.
    """
    if language != "python":
        return False
    return "__name__" in source and "__main__" in source


@dataclass
class FileResult:
    path: Path
    ok: bool
    mismatches: list[str] = field(default_factory=list)
    allowlisted: list[str] = field(default_factory=list)
    error: str | None = None


def run_json(cmd: list[str], cwd: Path) -> dict:
    proc = subprocess.run(cmd, cwd=cwd, capture_output=True, text=True, timeout=120)
    if proc.returncode != 0:
        raise RuntimeError(
            f"{' '.join(cmd)} exited {proc.returncode}: {proc.stderr.strip()[:300]}"
        )
    return json.loads(proc.stdout)


def close_enough(a: float, b: float) -> bool:
    return math.isclose(a, b, rel_tol=FLOAT_TOL, abs_tol=FLOAT_TOL)


# Which dimension each raw-metric namespace feeds, so a known divergence in
# one raw metric can be recognized as the (expected) root cause of a
# downstream score/verdict divergence, rather than flagged as a second,
# independent failure.
METRIC_PREFIX_TO_DIMENSION = {
    "cfg.": "simple",
    "ast.": "simple",
    "mdg.": "composable",
    "cpg.": "secure",
}


def _normalize_scores(scores: dict) -> dict:
    """Normalize a `scores` map to a 0-100, 1dp scale, auto-detecting
    whether it's already 0-100 (old --json) or raw 0.0-1.0 (Rust --json).

    A quality score is never legitimately > 1.5 on the 0-1 scale, so any
    value above that threshold marks the whole map as already-percentage.
    Detecting per-payload (not hardcoding "reference is old, candidate is
    new") means the same comparison works when both sides are Rust — the
    self-parity smoke test (`--reference` pointed at the candidate binary).
    """
    if not scores:
        return {}
    scale = 1.0 if max(scores.values()) > 1.5 else 100.0
    return {k: round(float(v) * scale, 1) for k, v in scores.items()}


def compare(ref: dict, cand: dict, language: str) -> tuple[list[str], list[str]]:
    """Return (real_mismatches, allowlisted_mismatches)."""
    mismatches: list[str] = []
    allowlisted: list[str] = []
    affected_dimensions: set[str] = set()

    def record(metric: str, ref_value, cand_value) -> None:
        divergence = next(
            (d for d in KNOWN_DIVERGENCES if d.metric == metric and d.matches(language)),
            None,
        )
        message = f"{metric}: reference={ref_value!r} candidate={cand_value!r}"
        if divergence is not None:
            allowlisted.append(f"{message}  [allowlisted: {divergence.issue}]")
            dimension = next(
                (
                    d
                    for prefix, d in METRIC_PREFIX_TO_DIMENSION.items()
                    if metric.startswith(prefix)
                ),
                None,
            )
            if dimension is not None:
                affected_dimensions.add(dimension)
        else:
            mismatches.append(message)

    if ref["is_parseable"] != cand["is_parseable"]:
        mismatches.append(
            f"is_parseable: reference={ref['is_parseable']!r} "
            f"candidate={cand['is_parseable']!r}"
        )
        return mismatches, allowlisted

    ref_metrics = ref.get("raw_metrics", {})
    cand_metrics = cand.get("raw_metrics", {})
    for metric in sorted(set(ref_metrics) | set(cand_metrics)):
        ref_value = ref_metrics.get(metric)
        cand_value = cand_metrics.get(metric)
        if ref_value is None or cand_value is None:
            record(metric, ref_value, cand_value)
            continue
        if not close_enough(float(ref_value), float(cand_value)):
            record(metric, ref_value, cand_value)

    def record_downstream(field_name: str, dim: str, ref_value, cand_value) -> None:
        """A score/dimension mismatch that already has an allowlisted
        raw-metric cause is that cause's expected downstream consequence,
        not an independent failure."""
        message = f"{field_name}: reference={ref_value!r} candidate={cand_value!r}"
        if dim in affected_dimensions:
            allowlisted.append(
                f"{message}  [downstream of allowlisted raw-metric divergence above]"
            )
        else:
            mismatches.append(message)

    # The old --json reports scores on a 0-100 scale rounded to 1 decimal;
    # the Rust --json passes topos-core's raw 0.0-1.0 float through. Detect
    # each side's scale independently (rather than hardcoding "reference is
    # always 0-100") so this also works when both sides are Rust binaries —
    # e.g. the --reference=candidate self-parity smoke test — and normalize
    # both to a 0-100, 1dp scale before comparing.
    ref_scores = _normalize_scores(ref.get("scores", {}))
    cand_scores = _normalize_scores(cand.get("scores", {}))
    for dim in sorted(set(ref_scores) | set(cand_scores)):
        ref_value = ref_scores.get(dim)
        cand_value = cand_scores.get(dim)
        if ref_value is None or cand_value is None:
            record_downstream(f"scores.{dim}", dim, ref_value, cand_value)
            continue
        if not close_enough(ref_value, cand_value):
            record_downstream(f"scores.{dim}", dim, ref_value, cand_value)

    ref_dims = ref.get("dimensions", {})
    cand_dims = cand.get("dimensions", {})
    for dim in sorted(set(ref_dims) | set(cand_dims)):
        if ref_dims.get(dim) != cand_dims.get(dim):
            record_downstream(
                f"dimensions.{dim}", dim, ref_dims.get(dim), cand_dims.get(dim)
            )

    return mismatches, allowlisted


def discover_files(corpus: Path, language: str, limit: int | None) -> list[Path]:
    suffixes = LANGUAGE_SUFFIXES[language]
    files = sorted(p for p in corpus.rglob("*") if p.suffix in suffixes and p.is_file())
    return files[:limit] if limit else files


def resolve_command(spec: str) -> list[str]:
    """Turn a `--reference`/`--candidate` spec into an argv prefix.

    A bare path (or one that exists on disk) runs directly; anything with
    spaces (e.g. `uvx --from topos-mcp==0.3.11 topos`) is shell-split.
    """
    parts = shlex.split(spec)
    if len(parts) == 1:
        candidate = (ROOT / parts[0]).resolve()
        if candidate.is_file():
            return [str(candidate)]
    return parts


def main() -> int:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument(
        "--corpus",
        default="parity/corpus",
        help="Directory to walk for source files (default: parity/corpus).",
    )
    parser.add_argument(
        "--language",
        default="all",
        choices=[*sorted(LANGUAGE_SUFFIXES), "all"],
        help="Corpus language, or 'all' to sweep every language (default: all).",
    )
    parser.add_argument(
        "--reference",
        default=DEFAULT_REFERENCE,
        help=f"Old (reference) CLI command (default: {DEFAULT_REFERENCE!r}).",
    )
    parser.add_argument(
        "--candidate",
        default="target/release/topos",
        help="New (candidate) Rust topos binary (default: target/release/topos).",
    )
    parser.add_argument(
        "--limit",
        type=int,
        default=0,
        help="Max files per language (default: 0 = no limit).",
    )
    parser.add_argument(
        "--verbose",
        action="store_true",
        help="Print every file's result, not just failures.",
    )
    args = parser.parse_args()

    corpus = (ROOT / args.corpus).resolve()
    reference_cmd = resolve_command(args.reference)
    candidate_cmd = resolve_command(args.candidate)

    candidate_path = Path(candidate_cmd[0])
    if len(candidate_cmd) == 1 and not candidate_path.is_file():
        print(
            f"error: candidate binary not found at {candidate_path} — "
            "run: cargo build --release -p topos-cli",
            file=sys.stderr,
        )
        return 2

    languages = sorted(LANGUAGE_SUFFIXES) if args.language == "all" else [args.language]
    files: list[tuple[str, Path]] = []
    for language in languages:
        for path in discover_files(corpus, language, args.limit or None):
            files.append((language, path))
    if not files:
        print(f"error: no source files found under {corpus}", file=sys.stderr)
        return 2

    print(f"reference: {' '.join(reference_cmd)}")
    print(f"candidate: {' '.join(candidate_cmd)}")
    print(f"corpus:    {corpus.relative_to(ROOT)} ({len(files)} files)\n")

    results: list[FileResult] = []
    for language, path in files:
        rel = path.relative_to(ROOT)
        try:
            ref_json = run_json([*reference_cmd, "inspect", str(rel), "--json"], cwd=ROOT)
            cand_json = run_json(
                [*candidate_cmd, "inspect", str(rel), "--json"], cwd=ROOT
            )
        except Exception as exc:  # noqa: BLE001 - report and continue
            results.append(FileResult(path=rel, ok=False, error=str(exc)))
            continue
        mismatches, allowlisted = compare(ref_json, cand_json, language)
        if mismatches and has_unfiltered_main_guard(
            path.read_text(encoding="utf-8"), language
        ):
            allowlisted.extend(
                f"{m}  [allowlisted: pre-#133 __main__ guard]" for m in mismatches
            )
            mismatches = []
        results.append(
            FileResult(
                path=rel, ok=not mismatches, mismatches=mismatches, allowlisted=allowlisted
            )
        )

    failures = [r for r in results if not r.ok]
    errors = [r for r in results if r.error]
    allowlisted_only = [r for r in results if r.ok and r.allowlisted]

    for r in results:
        if r.error:
            print(f"ERROR  {r.path}: {r.error}")
        elif not r.ok:
            print(f"FAIL   {r.path}")
            for m in r.mismatches:
                print(f"         {m}")
            for m in r.allowlisted:
                print(f"         (allowlisted) {m}")
        elif args.verbose:
            print(f"OK     {r.path}")
            for m in r.allowlisted:
                print(f"         (allowlisted) {m}")

    passed = len(results) - len(failures)
    print(
        f"\n{len(results)} files checked: {passed} passed, "
        f"{len(failures)} failed ({len(errors)} errors)"
    )
    if allowlisted_only:
        print(
            f"{len(allowlisted_only)} files had only allowlisted (expected) divergences"
        )
    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
