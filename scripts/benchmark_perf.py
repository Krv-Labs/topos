#!/usr/bin/env python3
"""Benchmark: topos-cli (Rust) vs topos-mcp (Python) wall-clock time.

Measures two things, since they have very different causes:

1. Single-file startup + evaluate cost (subprocess-per-file) — dominated
   by interpreter/process startup for the Python side, and shows the
   latency an agent sees when calling the CLI once per file.
2. Whole-corpus throughput (one process, one `evaluate -r <dir>` call
   per CLI) — dominated by actual parse+analyze work once startup cost
   is amortized across every file in the directory.

Usage:
    cargo build --release -p topos-cli
    uv run python3 scripts/benchmark_perf.py
    uv run python3 scripts/benchmark_perf.py --corpus topos/core --language python
    uv run python3 scripts/benchmark_perf.py --corpus crates/topos-core/src \
        --language rust
"""

from __future__ import annotations

import argparse
import statistics
import subprocess
import sys
import time
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


def discover_files(corpus: Path, language: str, limit: int | None) -> list[Path]:
    suffixes = LANGUAGE_SUFFIXES[language]
    files = sorted(p for p in corpus.rglob("*") if p.suffix in suffixes and p.is_file())
    return files[:limit] if limit else files


def timed_run(cmd: list[str], cwd: Path) -> float:
    start = time.perf_counter()
    proc = subprocess.run(cmd, cwd=cwd, capture_output=True, text=True, timeout=120)
    elapsed = time.perf_counter() - start
    if proc.returncode != 0:
        raise RuntimeError(
            f"{' '.join(cmd)} exited {proc.returncode}: {proc.stderr.strip()[:300]}"
        )
    return elapsed


def summarize(label: str, samples: list[float]) -> float:
    mean = statistics.mean(samples)
    median = statistics.median(samples)
    lo, hi = min(samples), max(samples)
    print(
        f"  {label:<28} mean={mean * 1000:8.1f}ms  median={median * 1000:8.1f}ms  "
        f"min={lo * 1000:8.1f}ms  max={hi * 1000:8.1f}ms  (n={len(samples)})"
    )
    return mean


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
        help="Python topos CLI to invoke (default: 'topos' on PATH)",
    )
    parser.add_argument(
        "--rust-bin",
        default="target/release/topos",
        help="Rust topos-cli binary (default: target/release/topos)",
    )
    parser.add_argument(
        "--repeats",
        type=int,
        default=8,
        help="Timed repetitions per benchmark (default: 8)",
    )
    parser.add_argument(
        "--single-file-sample",
        type=int,
        default=15,
        help="Individual files to time for the per-file benchmark (default: 15)",
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

    files = discover_files(corpus, args.language, None)
    if not files:
        print(f"error: no {args.language} files found under {corpus}", file=sys.stderr)
        return 2

    print(f"Corpus: {corpus.relative_to(ROOT)} ({len(files)} {args.language} files)")
    print()

    # --- Benchmark 1: single-file startup + evaluate, subprocess-per-file ---
    sample = files[: args.single_file_sample]
    print(
        f"1. Per-file cost (subprocess per file, {len(sample)} files, "
        f"{args.repeats} repeats each)"
    )
    py_samples = []
    rust_samples = []
    for path in sample:
        rel = str(path.relative_to(ROOT))
        for _ in range(args.repeats):
            py_samples.append(
                timed_run([args.python_bin, "inspect", rel, "--json"], cwd=ROOT)
            )
            rust_samples.append(
                timed_run([str(rust_bin), "inspect", rel, "--json"], cwd=ROOT)
            )
    py_mean = summarize("python (per invocation)", py_samples)
    rust_mean = summarize("rust (per invocation)", rust_samples)
    print(f"  -> rust is {py_mean / rust_mean:.1f}x faster per invocation")
    print()

    # --- Benchmark 2: whole-corpus throughput, one process per CLI ---
    print(
        f"2. Whole-corpus throughput (one 'evaluate -r' call over all "
        f"{len(files)} files, {args.repeats} repeats)"
    )
    py_corpus_samples = [
        timed_run(
            [
                args.python_bin,
                "evaluate",
                str(corpus.relative_to(ROOT)),
                "-r",
                "--language",
                args.language,
            ],
            cwd=ROOT,
        )
        for _ in range(args.repeats)
    ]
    rust_corpus_samples = [
        timed_run(
            [
                str(rust_bin),
                "evaluate",
                str(corpus.relative_to(ROOT)),
                "-r",
                "--language",
                args.language,
            ],
            cwd=ROOT,
        )
        for _ in range(args.repeats)
    ]
    py_corpus_mean = summarize("python (whole corpus)", py_corpus_samples)
    rust_corpus_mean = summarize("rust (whole corpus)", rust_corpus_samples)
    speedup = py_corpus_mean / rust_corpus_mean
    print(f"  -> rust is {speedup:.1f}x faster over the whole corpus")
    py_per_file = py_corpus_mean / len(files) * 1000
    rust_per_file = rust_corpus_mean / len(files) * 1000
    print(f"  -> python: {py_per_file:.2f}ms/file   rust: {rust_per_file:.2f}ms/file")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
