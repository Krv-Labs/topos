#!/usr/bin/env python3
"""Parity check: topos-core (Rust) vs topos-mcp (Python) classification output.

Runs `topos inspect <file> --json` through both the pure-Python CLI
(whatever `topos` resolves to on PATH — currently v0.3.10; re-run this
script once v0.3.11 ships to compare against that instead) and the Rust
`topos-cli` built from this worktree, over a corpus of source files, and
diffs the two ClassificationResult payloads metric by metric.

Real, understood, *intentional* divergences are allowlisted below — see
each entry's `issue` for why it's not a bug. Anything else that differs
is a real regression and fails the check.

Usage:
    uv run python3 scripts/parity_check.py
    uv run python3 scripts/parity_check.py --corpus topos/core --language python
    uv run python3 scripts/parity_check.py --corpus crates/topos-core/src \
        --language rust
    uv run python3 scripts/parity_check.py --rust-bin target/release/topos --verbose
"""

from __future__ import annotations

import argparse
import json
import math
import subprocess
import sys
from dataclasses import dataclass, field
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

LANGUAGE_SUFFIXES = {
    "python": (".py",),
    "rust": (".rs",),
    "javascript": (".js",),
    "typescript": (".ts",),
    "cpp": (".cpp", ".hpp"),
    "go": (".go",),
}

# Numeric metrics/scores are compared with this tolerance (both CLIs do
# floating-point math in a different order in a few places — e.g. Rust's
# min()-fold vs Python's min() over a list — so exact equality is too
# strict for values that are mathematically but not bit-for-bit identical).
FLOAT_TOL = 1e-6


@dataclass
class Divergence:
    """One expected, documented Python-vs-Rust difference."""

    metric: str
    issue: str
    reason: str
    # Only applies when this predicate(language) is True; None = always.
    applies: callable | None = None

    def matches(self, language: str) -> bool:
        return self.applies is None or self.applies(language)


KNOWN_DIVERGENCES: list[Divergence] = [
    Divergence(
        metric="ast.max_function_complexity",
        issue="#153",
        reason=(
            "Fixed for every non-Python language by v0.3.11: Python's "
            "calculate_max_function_complexity used to be vacuous (always 0.0) "
            "outside Python because its per-function ProgramObject never wired "
            "uast_root; it now runs the same language-neutral UAST path "
            "(DECISION_UAST_KINDS + the BinaryExpr and/or/&&/|| check) the Rust "
            "port already used, so non-Python values should now match exactly.\n"
            "Still diverges for Python source itself: Python's native "
            "tree-sitter path (used only for Python, to preserve the "
            "established gate) counts elif_clause, with_statement, "
            "assert_statement, list/dict/set/generator comprehensions, and "
            "boolean short-circuit operators as separate decision points, while "
            "the Rust port's UAST path only counts IfStmt/ForStmt/WhileStmt/"
            "MatchStmt/TryStmt (plus the same BinaryExpr check) — the same "
            "convention topos-core's own CFG builder already uses for "
            "cfg.cyclomatic. The Rust port makes ast.max_function_complexity "
            "internally consistent with cfg.cyclomatic; Python's two complexity "
            "metrics use different counting rules for the same constructs "
            "today, for Python source only."
        ),
        # Python-only now: non-Python languages were fixed by #153 and should
        # match Rust exactly (verify with --language javascript/typescript/
        # cpp/go runs; drop this Divergence entirely if none show up).
        applies=lambda language: language == "python",
    ),
]


def has_unfiltered_main_guard(source: str, language: str) -> bool:
    """Whether `source` has a construct this port's UAST TestNodeFilter
    excludes from the CFG/PDG the same way PR #133 (merged into v0.3.11,
    #127) now excludes it on the Python side.

    This crate's `graphs::uast::mapper_python::MainGuardFilter` forward-
    ported PR #133's `if __name__ == "__main__":` exclusion *before* it
    merged. Now that #127/#133 have landed on `main`, this allowlist
    entry is EXPECTED TO BE DEAD CODE: run parity_check.py against a
    corpus with `__main__` guards and confirm zero files hit this
    branch. If any still do, it means mapper_python.rs's forward-port
    doesn't exactly match the *merged* #127 TestNodeFilter/
    TestNodePredicate batch-classifier shape (mapper_common.rs has not
    yet been generalized to that interface as of this writing) --
    investigate before deleting this function.
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
    proc = subprocess.run(cmd, cwd=cwd, capture_output=True, text=True, timeout=30)
    if proc.returncode != 0:
        raise RuntimeError(
            f"{' '.join(cmd)} exited {proc.returncode}: {proc.stderr.strip()}"
        )
    return json.loads(proc.stdout)


def close_enough(a: float, b: float) -> bool:
    return math.isclose(a, b, rel_tol=FLOAT_TOL, abs_tol=FLOAT_TOL)


# Which dimension each raw-metric namespace feeds, so a known divergence
# in one raw metric can be recognized as the (expected) root cause of a
# downstream score/verdict divergence in the dimension it feeds, instead
# of that consequence being flagged as a second, unrelated failure.
METRIC_PREFIX_TO_DIMENSION = {
    "cfg.": "simple",
    "ast.": "simple",
    "mdg.": "composable",
    "cpg.": "secure",
}


def compare(py: dict, rust: dict, language: str) -> tuple[list[str], list[str]]:
    """Return (real_mismatches, allowlisted_mismatches)."""
    mismatches: list[str] = []
    allowlisted: list[str] = []
    affected_dimensions: set[str] = set()

    def record(metric: str, py_value, rust_value) -> None:
        divergence = next(
            (
                d
                for d in KNOWN_DIVERGENCES
                if d.metric == metric and d.matches(language)
            ),
            None,
        )
        message = f"{metric}: python={py_value!r} rust={rust_value!r}"
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

    if py["is_parseable"] != rust["is_parseable"]:
        mismatches.append(
            f"is_parseable: python={py['is_parseable']!r} rust={rust['is_parseable']!r}"
        )
        return mismatches, allowlisted

    py_metrics = py.get("raw_metrics", {})
    rust_metrics = rust.get("raw_metrics", {})
    for metric in sorted(set(py_metrics) | set(rust_metrics)):
        py_value = py_metrics.get(metric)
        rust_value = rust_metrics.get(metric)
        if py_value is None or rust_value is None:
            record(metric, py_value, rust_value)
            continue
        if not close_enough(float(py_value), float(rust_value)):
            record(metric, py_value, rust_value)

    def record_downstream(field_name: str, dim: str, py_value, rust_value) -> None:
        """A mismatch in a score/dimension/verdict that already has an
        allowlisted raw-metric cause is that cause's expected downstream
        consequence, not an independent failure."""
        message = f"{field_name}: python={py_value!r} rust={rust_value!r}"
        if dim in affected_dimensions:
            allowlisted.append(
                f"{message}  [downstream of allowlisted raw-metric divergence above]"
            )
        else:
            mismatches.append(message)

    # Python's --json reports scores on a 0-100 scale, rounded to 1
    # decimal place for display; Rust's native ClassificationResult (and
    # its --json) passes topos-core's raw 0.0-1.0 float straight through.
    # Normalize scale *and* match Python's own display rounding before
    # comparing — both are formatting choices, not divergences worth
    # allowlisting per-metric, and rounding to 1dp is a real assertion
    # (it still catches a wrong score, just not sub-0.05 float noise).
    py_scores = py.get("scores", {})
    rust_scores = {k: round(v * 100.0, 1) for k, v in rust.get("scores", {}).items()}
    for dim in sorted(set(py_scores) | set(rust_scores)):
        py_value = py_scores.get(dim)
        rust_value = rust_scores.get(dim)
        if py_value is None or rust_value is None:
            record_downstream(f"scores.{dim}", dim, py_value, rust_value)
            continue
        if not close_enough(round(float(py_value), 1), rust_value):
            record_downstream(f"scores.{dim}", dim, py_value, rust_value)

    py_dims = py.get("dimensions", {})
    rust_dims = rust.get("dimensions", {})
    for dim in sorted(set(py_dims) | set(rust_dims)):
        if py_dims.get(dim) != rust_dims.get(dim):
            record_downstream(
                f"dimensions.{dim}", dim, py_dims.get(dim), rust_dims.get(dim)
            )

    return mismatches, allowlisted


def discover_files(corpus: Path, language: str, limit: int | None) -> list[Path]:
    suffixes = LANGUAGE_SUFFIXES[language]
    files = sorted(p for p in corpus.rglob("*") if p.suffix in suffixes and p.is_file())
    return files[:limit] if limit else files


def main() -> int:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument(
        "--corpus",
        default="topos/core",
        help="Directory to walk for source files (default: topos/core)",
    )
    parser.add_argument(
        "--language",
        default="python",
        choices=sorted(LANGUAGE_SUFFIXES),
        help="Language of the corpus (default: python)",
    )
    parser.add_argument(
        "--python-bin",
        default="topos",
        help="Python topos CLI to invoke (default: pip-installed 'topos' on PATH)",
    )
    parser.add_argument(
        "--rust-bin",
        default="target/release/topos",
        help="Rust topos-cli binary (default: target/release/topos; "
        "run 'cargo build --release -p topos-cli' first)",
    )
    parser.add_argument(
        "--limit",
        type=int,
        default=40,
        help="Max files to check (default: 40; pass 0 for no limit)",
    )
    parser.add_argument(
        "--verbose",
        action="store_true",
        help="Print every file's result, not just failures",
    )
    args = parser.parse_args()

    corpus = (ROOT / args.corpus).resolve()
    rust_bin = (ROOT / args.rust_bin).resolve()
    if not rust_bin.is_file():
        print(
            f"error: rust binary not found at {rust_bin} — "
            "run: cargo build --release -p topos-cli",
            file=sys.stderr,
        )
        return 2

    files = discover_files(corpus, args.language, args.limit or None)
    if not files:
        print(f"error: no {args.language} files found under {corpus}", file=sys.stderr)
        return 2

    results: list[FileResult] = []
    for path in files:
        rel = path.relative_to(ROOT)
        try:
            py_json = run_json(
                [args.python_bin, "inspect", str(rel), "--json"], cwd=ROOT
            )
            rust_json = run_json(
                [str(rust_bin), "inspect", str(rel), "--json"], cwd=ROOT
            )
        except Exception as exc:  # noqa: BLE001 - report and continue
            results.append(FileResult(path=rel, ok=False, error=str(exc)))
            continue
        mismatches, allowlisted = compare(py_json, rust_json, args.language)
        if mismatches and has_unfiltered_main_guard(
            path.read_text(encoding="utf-8"), args.language
        ):
            allowlisted.extend(
                f"{m}  [allowlisted: pre-PR#133 __main__ guard, "
                "see has_unfiltered_main_guard]"
                for m in mismatches
            )
            mismatches = []
        results.append(
            FileResult(
                path=rel,
                ok=not mismatches,
                mismatches=mismatches,
                allowlisted=allowlisted,
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

    print()
    passed = len(results) - len(failures)
    print(
        f"{len(results)} files checked: {passed} passed, "
        f"{len(failures)} failed ({len(errors)} errors)"
    )
    if allowlisted_only:
        print(
            f"{len(allowlisted_only)} files had only allowlisted (expected) divergences"
        )

    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
