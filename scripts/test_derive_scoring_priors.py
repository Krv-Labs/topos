#!/usr/bin/env python3
"""Self-check for derive_scoring_priors.py on synthetic data."""

from __future__ import annotations

import random
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import derive_scoring_priors as d  # noqa: E402


def main() -> int:
    # Ties: exact P(X < v) and P(X <= v).
    pts = d.weighted_points([(x, 1.0) for x in (1, 2, 2, 3)])
    assert pts == [[1.0, 0.0, 0.25], [2.0, 0.25, 0.75], [3.0, 0.75, 1.0]], pts

    # Cap at 256 points, keeping min and max.
    pts = d.weighted_points([(float(x), 1.0) for x in range(10_000)])
    assert len(pts) <= 256 and pts[0][0] == 0.0 and pts[-1] == [9999.0, 0.9999, 1.0], pts[-1]
    assert all(a[0] < b[0] for a, b in zip(pts, pts[1:]))

    # Two clearly separated groups: between-group variance dominates, k is small.
    rng = random.Random(0)
    groups = {"a": [rng.uniform(1, 2) for _ in range(30)], "b": [rng.uniform(500, 600) for _ in range(30)]}
    assert d.shrinkage_k(groups) == d.K_MIN, d.shrinkage_k(groups)
    # Identical groups: no between-group signal, k clamps high.
    same = {g: [1.0, 2.0, 3.0, 4.0, 5.0] for g in "abc"}
    assert d.shrinkage_k(same) == d.K_MAX

    # Equal-language weighting: 1 row of 10 vs 99 rows of 0 pools to 50/50.
    pooled = [(10.0, 1.0)] + [(0.0, 1.0 / 99)] * 99
    pts = d.weighted_points(pooled)
    assert pts == [[0.0, 0.0, 0.5], [10.0, 0.5, 1.0]], pts
    assert d.weighted_median(pooled) == 0.0

    print("derive_scoring_priors self-check OK")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
