#!/usr/bin/env python3
"""Derive per-language priors for ADVISORY metric scoring.

Topos scores advisory metrics relative to the user's codebase, shrunk toward a
per-language prior:  F~ = w * F_local + (1 - w) * F_lang,  w = n / (n + k).
This script computes F_lang (mid-rank CDF tables) and k (shrinkage constant)
once from existing benchmark runs and writes
topos/engine/src/evaluation/advisory_priors.json. Rerun it only when a metric
definition changes or after a leaderboard rerun.

Inputs (each repeatable):
  --raw-dir DIR          topos-leaderboard structural_scores_*.jsonl files.
                         Rows older than --min-topos-version (or without a
                         version) are dropped; malformed lines are skipped.
                         Group key for k: package.
  --evaluate-json FILE   output of `topos evaluate <dir> -r --json`. Group key
                         for k: the file stem, or GROUP when given as
                         GROUP=FILE.

Method:
  * Unparseable rows are skipped. mdg.instability only uses rows with
    mdg.coupling >= 2 (below that it is an unresolvable 0.5 fallback).
  * Per-language tables use the raw sample and are emitted only when n >= 50.
  * `_all` pools the emitted languages with EQUAL WEIGHT PER LANGUAGE: each
    language contributes total weight 1 to a weighted ECDF, so one large
    ecosystem cannot dominate the pooled prior. Its median is the weighted
    median.
  * points are ascending [value, P(X < value), P(X <= value)], exact on the
    sample. With more than 256 distinct values, 256 values are taken at evenly
    spaced quantile ranks (min and max always included).
  * k: method of moments on y = log1p(x) over groups with >= 5 rows.
    sigma2_within = pooled within-group variance,
    tau2 = max(var(group means) - mean(sigma2_within / n_g), tiny),
    k = sigma2_within / tau2, clamped to [1, 50].
  * A metric is `provisional` when none of its rows come from leaderboard
    sources.
"""

from __future__ import annotations

import argparse
import json
import math
import statistics
import sys
from collections import defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DEFAULT_OUTPUT = ROOT / "topos" / "engine" / "src" / "evaluation" / "advisory_priors.json"
METRICS = (
    "cfg.cyclomatic",
    "cfg.nesting_depth",
    "cfg.essential",
    "mdg.fan_in",
    "mdg.instability",
)
FLAG_PERCENTILE = 0.9
MIN_LANG_ROWS = 50
MIN_GROUP_ROWS = 5
MAX_POINTS = 256
K_MIN, K_MAX = 1.0, 50.0
TINY = 1e-9


def parse_version(text: object) -> tuple[int, ...] | None:
    try:
        return tuple(int(p) for p in str(text).split("-")[0].split("."))
    except ValueError:
        return None


def sig6(x: float) -> float:
    return float(f"{x:.6g}")


def metric_values(raw: dict) -> dict[str, float]:
    """Advisory metric values usable from one row's raw_metrics."""
    out = {}
    for m in METRICS:
        v = raw.get(m)
        if not isinstance(v, (int, float)) or isinstance(v, bool) or not math.isfinite(v):
            continue
        if m == "mdg.instability" and not raw.get("mdg.coupling", 0) >= 2:
            continue
        out[m] = float(v)
    return out


def load_raw_dir(directory: Path, min_version: tuple[int, ...]):
    """Yield (source, rows) per jsonl file; rows are (language, group, values)."""
    for path in sorted(directory.glob("structural_scores_*.jsonl")):
        rows, versions = [], set()
        with path.open(encoding="utf-8") as f:
            for line in f:
                try:
                    r = json.loads(line)
                except ValueError:
                    continue
                if not isinstance(r, dict) or not r.get("is_parseable"):
                    continue
                v = parse_version(r.get("topos_version"))
                raw = r.get("raw_metrics")
                if v is None or v < min_version or not isinstance(raw, dict):
                    continue
                vals = metric_values(raw)
                if vals and r.get("language"):
                    rows.append((r["language"], ("leaderboard", str(r.get("package"))), vals))
                    versions.add(r["topos_version"])
        if rows:
            src = {
                "kind": "leaderboard-jsonl",
                "path": f"{directory.name}/{path.name}",
                "rows": len(rows),
                "topos_versions": sorted(versions),
            }
            yield src, rows


def load_evaluate_json(arg: str):
    group, sep, file = arg.partition("=")
    if not sep or "/" in group:
        group, file = "", arg
    path = Path(file)
    group = group or path.stem
    data = json.loads(path.read_text(encoding="utf-8"))
    rows = []
    for r in data.get("results", []):
        if not isinstance(r, dict) or not r.get("is_parseable") or not r.get("language"):
            continue
        vals = metric_values(r.get("raw_metrics") or {})
        if vals:
            rows.append((r["language"], ("evaluate", group), vals))
    return {"kind": "evaluate-json", "path": path.name, "rows": len(rows)}, rows


def weighted_points(samples: list[tuple[float, float]]) -> list[list[float]]:
    """Exact [value, cdf_lt, cdf_le] for (value, weight) samples, capped at MAX_POINTS."""
    total = sum(w for _, w in samples)
    by_value: dict[float, float] = defaultdict(float)
    for v, w in samples:
        by_value[v] += w
    pts, acc = [], 0.0
    for v in sorted(by_value):
        lt = acc / total
        acc += by_value[v]
        pts.append((v, lt, acc / total))
    if len(pts) > MAX_POINTS:
        chosen, j = [], 0
        for i in range(MAX_POINTS - 1):
            q = i / (MAX_POINTS - 1)
            while j < len(pts) - 1 and pts[j][2] < q - 1e-12:
                j += 1
            if not chosen or chosen[-1] is not pts[j]:
                chosen.append(pts[j])
        if chosen[-1] is not pts[-1]:
            chosen.append(pts[-1])
        pts = chosen
    out: list[list[float]] = []
    for v, lt, le in pts:
        v, lt, le = sig6(v), round(lt, 6), round(le, 6)
        if out and out[-1][0] == v:  # values merged by rounding
            out[-1][2] = le
        else:
            out.append([v, lt, le])
    return out


def weighted_median(samples: list[tuple[float, float]]) -> float:
    total = sum(w for _, w in samples)
    acc = 0.0
    for v, w in sorted(samples):
        acc += w
        if acc >= total / 2 - 1e-12:
            return v
    return samples[-1][0]


def shrinkage_k(groups: dict[object, list[float]]) -> float | None:
    """Method-of-moments k = sigma2_within / tau2 on log1p values."""
    ys = [[math.log1p(x) for x in xs] for xs in groups.values() if len(xs) >= MIN_GROUP_ROWS]
    if len(ys) < 2:
        return None
    dof = sum(len(y) - 1 for y in ys)
    within = sum(statistics.variance(y) * (len(y) - 1) for y in ys) / dof
    means = [statistics.fmean(y) for y in ys]
    tau2 = max(statistics.variance(means) - statistics.fmean(within / len(y) for y in ys), TINY)
    return min(max(within / tau2, K_MIN), K_MAX)


def build(raw_dirs, evaluate_jsons, min_version) -> tuple[dict, str]:
    sources, rows = [], []
    for d in raw_dirs:
        for src, rs in load_raw_dir(Path(d), min_version):
            sources.append(src)
            rows.extend(rs)
    for e in evaluate_jsons:
        src, rs = load_evaluate_json(e)
        sources.append(src)
        rows.extend(rs)

    metrics, provisional, summary = {}, [], []
    for m in METRICS:
        by_lang: dict[str, list[float]] = defaultdict(list)
        by_group: dict[object, list[float]] = defaultdict(list)
        from_leaderboard = False
        for lang, group, vals in rows:
            if m in vals:
                by_lang[lang].append(vals[m])
                by_group[group].append(vals[m])
                from_leaderboard |= group[0] == "leaderboard"
        if not by_lang:
            continue
        if not from_leaderboard:
            provisional.append(m)
        languages, pooled = {}, []
        for lang in sorted(by_lang):
            xs = by_lang[lang]
            if len(xs) < MIN_LANG_ROWS:
                continue
            languages[lang] = {
                "n": len(xs),
                "median": sig6(statistics.median(xs)),
                "points": weighted_points([(x, 1.0) for x in xs]),
            }
            pooled.extend((x, 1.0 / len(xs)) for x in xs)
        if pooled:
            languages["_all"] = {
                "n": len(pooled),
                "median": sig6(weighted_median(pooled)),
                "points": weighted_points(pooled),
            }
        k = shrinkage_k(by_group)
        metrics[m] = {
            "k": round(k if k is not None else K_MAX, 3),
            "flag_percentile": FLAG_PERCENTILE,
            "languages": languages,
        }
        counts = ", ".join(f"{lang}={len(by_lang[lang])}" for lang in sorted(by_lang))
        summary.append(f"{m}: k={metrics[m]['k']} groups>={MIN_GROUP_ROWS}="
                       f"{sum(1 for g in by_group.values() if len(g) >= MIN_GROUP_ROWS)} rows: {counts}")

    doc = {
        "schema": 1,
        "generated_by": "scripts/derive_scoring_priors.py",
        "sources": sources,
        "provisional": provisional,
        "metrics": metrics,
    }
    return doc, "\n".join(summary)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--raw-dir", action="append", default=[], help="leaderboard raw dir (repeatable)")
    ap.add_argument("--evaluate-json", action="append", default=[],
                    help="`topos evaluate -r --json` output, optionally GROUP=FILE (repeatable)")
    ap.add_argument("--min-topos-version", default="0.5.0")
    ap.add_argument("--output", type=Path, default=DEFAULT_OUTPUT)
    ap.add_argument("--check", action="store_true", help="exit 1 if --output is out of date")
    args = ap.parse_args()
    if not args.raw_dir and not args.evaluate_json:
        ap.error("need at least one --raw-dir or --evaluate-json")
    min_version = parse_version(args.min_topos_version)
    if min_version is None:
        ap.error(f"bad --min-topos-version: {args.min_topos_version}")

    doc, summary = build(args.raw_dir, args.evaluate_json, min_version)
    text = json.dumps(doc, indent=1, sort_keys=True) + "\n"
    print(summary, file=sys.stderr)
    if args.check:
        current = args.output.read_text(encoding="utf-8") if args.output.exists() else ""
        if current != text:
            print(f"{args.output} is out of date; rerun scripts/derive_scoring_priors.py", file=sys.stderr)
            return 1
        print(f"{args.output} is up to date", file=sys.stderr)
        return 0
    args.output.write_text(text, encoding="utf-8")
    print(f"wrote {args.output}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
