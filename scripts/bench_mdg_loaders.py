#!/usr/bin/env python3
"""Benchmark Python ladybug vs Rust `from_lbug_path` (native lbug + cypher fallback).

Reports:
  - python_ladybug_full: all NODE labels (skip CodeEmbedding) + all CodeRelation
    — closest to old Topos `_from_ladybugdb`
  - python_ladybug_composable: File nodes + CodeRelation only — closest to the
    Rust COMPOSABLE shape (File + edges + stubs)
  - rust_from_lbug_path: native `lbug` primary via the bench example binary
"""
from __future__ import annotations

import argparse
import statistics
import subprocess
import sys
import time
from pathlib import Path


def _open_conn(lbug_path: Path):
    import ladybug as lb

    try:
        db = lb.Database(str(lbug_path), read_only=True)
    except RuntimeError:
        db = lb.Database(str(lbug_path), read_only=False)
    return lb.Connection(db)


def load_ladybug_full(lbug_path: Path) -> tuple[int, int]:
    conn = _open_conn(lbug_path)
    tables_result = conn.execute("CALL show_tables() RETURN *")
    node_tables = []
    while tables_result.has_next():
        row = tables_result.get_next()
        if len(row) >= 3 and row[2] == "NODE":
            node_tables.append(row[1])

    nodes = 0
    for label in node_tables:
        if label == "CodeEmbedding":
            continue
        result = conn.execute(f"MATCH (n:`{label}`) RETURN n.id")
        while result.has_next():
            result.get_next()
            nodes += 1

    try:
        result = conn.execute(
            "MATCH (src)-[r:CodeRelation]->(dst) "
            "RETURN src.id, dst.id, r.type, r.confidence, r.reason, r.step"
        )
    except RuntimeError:
        result = conn.execute(
            "MATCH (src)-[r:CodeRelation]->(dst) "
            "RETURN src.id, dst.id, r.type, r.confidence, r.reason"
        )
    rels = 0
    while result.has_next():
        result.get_next()
        rels += 1
    return nodes, rels


def load_ladybug_composable(lbug_path: Path) -> tuple[int, int]:
    """Same data shape as the Rust cypher loader: File + relationships."""
    conn = _open_conn(lbug_path)
    nodes = 0
    result = conn.execute(
        "MATCH (n:`File`) RETURN n.id, n.filePath, n.name"
    )
    while result.has_next():
        result.get_next()
        nodes += 1

    try:
        result = conn.execute(
            "MATCH (src)-[r:CodeRelation]->(dst) "
            "RETURN src.id, dst.id, r.type, r.step"
        )
    except RuntimeError:
        result = conn.execute(
            "MATCH (src)-[r:CodeRelation]->(dst) RETURN src.id, dst.id, r.type"
        )
    rels = 0
    while result.has_next():
        result.get_next()
        rels += 1
    return nodes, rels


def time_runs(fn, repeats: int):
    out = fn()  # warmup
    samples = []
    for _ in range(repeats):
        t0 = time.perf_counter()
        out = fn()
        samples.append(time.perf_counter() - t0)
    return samples, out


def summarize(name: str, samples: list[float], extra: str = "") -> None:
    mean = statistics.mean(samples)
    p50 = statistics.median(samples)
    print(
        f"{name}: n={len(samples)} mean={mean:.3f}s median={p50:.3f}s "
        f"min={min(samples):.3f}s max={max(samples):.3f}s{extra}"
    )


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--lbug", type=Path, required=True)
    ap.add_argument("--project-root", type=Path, required=True)
    ap.add_argument("--repeats", type=int, default=3)
    ap.add_argument("--rust-bin", type=Path, default=None)
    args = ap.parse_args()

    if not args.lbug.is_file():
        print(f"not a binary lbug store: {args.lbug}", file=sys.stderr)
        return 1

    import ladybug

    ver = getattr(ladybug, "__version__", "?")
    print(f"store={args.lbug} size_mb={args.lbug.stat().st_size / 1e6:.1f}")
    print(f"ladybug={ver}")
    print(f"repeats={args.repeats} (plus 1 warmup each)")

    samples, (nodes, rels) = time_runs(
        lambda: load_ladybug_full(args.lbug), args.repeats
    )
    summarize("python_ladybug_full", samples, f" nodes={nodes} rels={rels}")

    samples, (nodes, rels) = time_runs(
        lambda: load_ladybug_composable(args.lbug), args.repeats
    )
    summarize("python_ladybug_composable", samples, f" nodes={nodes} rels={rels}")

    if args.rust_bin and args.rust_bin.exists():
        def run_rust():
            proc = subprocess.run(
                [str(args.rust_bin), str(args.project_root), str(args.lbug)],
                check=True,
                capture_output=True,
                text=True,
            )
            return proc.stdout.strip()

        samples, out = time_runs(run_rust, args.repeats)
        summarize("rust_from_lbug_path", samples, f" {out}")
    else:
        print("rust_from_lbug_path: skipped (pass --rust-bin)")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
