"""
Directed Forman-Ricci curvature perf benchmark (issue #86 acceptance
criterion: |V|=10k, |E|=50k in <100ms).

Opt-in via TOPOS_BENCHMARK=1, matching the other benchmarks in this
directory — this exercises the Rust extension through the full Python/PyO3
call boundary (the realistic path a user hits), not just the pure-Rust
timing already asserted inline in ``src/frc.rs``'s test suite.

The <100ms acceptance bar is a release-build target. Under a `maturin
develop` debug build (the default for local/dev venvs and this repo's CI)
the extension is unoptimized and commonly 10-30x slower for numeric code, so
this test uses a generous margin by default and only enforces the literal
100ms bound when ``TOPOS_BENCHMARK_RELEASE=1`` signals a release wheel is
installed (e.g. after ``uv run maturin develop --release``).

Local usage:

  # Debug build (generous margin, sanity-check only)
  TOPOS_BENCHMARK=1 uv run pytest tests/benchmarks/test_curvature_perf.py -s --no-cov

  # Release build (enforces the literal <100ms acceptance criterion)
  uv run maturin develop --release
  TOPOS_BENCHMARK=1 TOPOS_BENCHMARK_RELEASE=1 \\
    uv run pytest tests/benchmarks/test_curvature_perf.py -s --no-cov
"""

from __future__ import annotations

import os
import random
import time

import pytest

pytestmark = pytest.mark.skipif(
    os.environ.get("TOPOS_BENCHMARK") != "1",
    reason="Set TOPOS_BENCHMARK=1 to run performance benchmarks.",
)


def test_directed_curvature_10k_nodes_50k_edges():
    from topos.topos_functors import WeightedEdge, directed_forman_curvature

    n = 10_000
    e_count = 50_000
    rng = random.Random(42)
    edges = []
    for _ in range(e_count):
        s = rng.randrange(n)
        t = rng.randrange(n)
        if t == s:
            t = (t + 1) % n
        edges.append(WeightedEdge(s, t, 1.0))

    start = time.perf_counter()
    results = directed_forman_curvature(edges, None)
    elapsed_ms = (time.perf_counter() - start) * 1000

    print(f"\ndirected_forman_curvature(10k/50k) took {elapsed_ms:.1f}ms")
    assert len(results) == e_count

    if os.environ.get("TOPOS_BENCHMARK_RELEASE") == "1":
        assert elapsed_ms < 100, (
            f"took {elapsed_ms:.1f}ms, expected <100ms on a release build "
            "(issue #86 acceptance criterion)"
        )
    else:
        # Debug-build sanity bound: catches an algorithmic blowup (e.g. an
        # accidental O(V^2)/O(E^2) regression), not a strict SLA.
        assert elapsed_ms < 5000, (
            f"took {elapsed_ms:.1f}ms on a debug build — investigate for a "
            "real algorithmic regression"
        )
