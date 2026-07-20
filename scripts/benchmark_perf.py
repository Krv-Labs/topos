#!/usr/bin/env python3
"""Benchmark: the new Rust `topos` CLI vs the last published Python release.

Measures two things, because they have very different causes:

1. Per-invocation cost (subprocess-per-file) — dominated by interpreter /
   process startup on the Python side. This is the latency an agent or
   editor sees when it calls the CLI once per file, and it is where the
   Rust rewrite wins biggest. This is the headline number to advertise.
2. Whole-corpus throughput (one `evaluate -r <dir>` call per CLI) —
   startup amortized across every file, so this isolates raw parse+analyze
   speed once the interpreter is already warm.

Both sides are external processes; nothing is imported from `topos`. The
reference is whatever `--reference` resolves to (default: the last Python
release from PyPI via `uvx`); the candidate is the Rust binary.

Usage:
    cargo build --release -p topos-cli
    python3 scripts/benchmark_perf.py                       # bundled corpus, python
    python3 scripts/benchmark_perf.py --corpus crates/topos-core/src --language rust
    python3 scripts/benchmark_perf.py --reference "uvx --from topos-mcp==0.3.11 topos"
    python3 scripts/benchmark_perf.py --candidate-only      # skip the reference
"""

from __future__ import annotations

import argparse
import shlex
import statistics
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

DEFAULT_REFERENCE = "uvx --from topos-mcp==0.3.11 topos"

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


def resolve_command(spec: str) -> list[str]:
    parts = shlex.split(spec)
    if len(parts) == 1:
        candidate = (ROOT / parts[0]).resolve()
        if candidate.is_file():
            return [str(candidate)]
    return parts


def timed_run(cmd: list[str], cwd: Path) -> float:
    start = time.perf_counter()
    proc = subprocess.run(cmd, cwd=cwd, capture_output=True, text=True, timeout=300)
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
    return median


def warmup(cmd: list[str], path: str) -> None:
    """One untimed run to page in the binary / prime uvx's tool cache."""
    try:
        subprocess.run(
            [*cmd, "inspect", path, "--json"],
            cwd=ROOT,
            capture_output=True,
            text=True,
            timeout=300,
        )
    except Exception:  # noqa: BLE001 - warmup is best-effort
        pass


def main() -> int:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument("--corpus", default="parity/corpus")
    parser.add_argument(
        "--language", default="python", choices=sorted(LANGUAGE_SUFFIXES)
    )
    parser.add_argument("--reference", default=DEFAULT_REFERENCE)
    parser.add_argument("--candidate", default="target/release/topos")
    parser.add_argument(
        "--candidate-only",
        action="store_true",
        help="Only time the candidate (no reference / no speedup ratio).",
    )
    parser.add_argument("--repeats", type=int, default=8)
    parser.add_argument("--single-file-sample", type=int, default=12)
    args = parser.parse_args()

    corpus = (ROOT / args.corpus).resolve()
    candidate_cmd = resolve_command(args.candidate)
    reference_cmd = None if args.candidate_only else resolve_command(args.reference)

    candidate_path = Path(candidate_cmd[0])
    if len(candidate_cmd) == 1 and not candidate_path.is_file():
        print(
            f"error: candidate binary not found at {candidate_path} — "
            "run: cargo build --release -p topos-cli",
            file=sys.stderr,
        )
        return 2

    files = discover_files(corpus, args.language, None)
    if not files:
        print(f"error: no {args.language} files found under {corpus}", file=sys.stderr)
        return 2

    corpus_rel = str(corpus.relative_to(ROOT))
    print(f"corpus:    {corpus_rel} ({len(files)} {args.language} files)")
    print(f"candidate: {' '.join(candidate_cmd)}")
    if reference_cmd:
        print(f"reference: {' '.join(reference_cmd)}")
    print()

    sample = files[: args.single_file_sample]
    first = str(sample[0].relative_to(ROOT))
    warmup(candidate_cmd, first)
    if reference_cmd:
        warmup(reference_cmd, first)

    # --- Benchmark 1: per-invocation (subprocess per file) ---
    print(
        f"1. Per-invocation cost (subprocess per file, {len(sample)} files, "
        f"{args.repeats} repeats each)"
    )
    cand_samples: list[float] = []
    ref_samples: list[float] = []
    for path in sample:
        rel = str(path.relative_to(ROOT))
        for _ in range(args.repeats):
            cand_samples.append(timed_run([*candidate_cmd, "inspect", rel, "--json"], ROOT))
            if reference_cmd:
                ref_samples.append(
                    timed_run([*reference_cmd, "inspect", rel, "--json"], ROOT)
                )
    cand_med = summarize("rust (per invocation)", cand_samples)
    if reference_cmd:
        ref_med = summarize("python (per invocation)", ref_samples)
        print(f"  -> rust is {ref_med / cand_med:.1f}x faster per invocation")
    print()

    # --- Benchmark 2: whole-corpus throughput (one process per CLI) ---
    print(
        f"2. Whole-corpus throughput (one 'evaluate -r' over {len(files)} files, "
        f"{args.repeats} repeats)"
    )
    evaluate_argv = ["evaluate", corpus_rel, "-r", "--language", args.language]
    cand_corpus = [timed_run([*candidate_cmd, *evaluate_argv], ROOT) for _ in range(args.repeats)]
    cand_corpus_med = summarize("rust (whole corpus)", cand_corpus)
    if reference_cmd:
        ref_corpus = [
            timed_run([*reference_cmd, *evaluate_argv], ROOT) for _ in range(args.repeats)
        ]
        ref_corpus_med = summarize("python (whole corpus)", ref_corpus)
        print(f"  -> rust is {ref_corpus_med / cand_corpus_med:.1f}x faster over the corpus")
        print(
            f"  -> python: {ref_corpus_med / len(files) * 1000:.2f}ms/file   "
            f"rust: {cand_corpus_med / len(files) * 1000:.2f}ms/file"
        )
    else:
        print(f"  -> rust: {cand_corpus_med / len(files) * 1000:.2f}ms/file")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
