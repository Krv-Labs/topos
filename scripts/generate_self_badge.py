#!/usr/bin/env python3
"""Evaluate the Topos repo and write a shields-style README badge SVG.

Scores ``topos/cli``, ``topos/engine``, and ``topos/mcp`` with the local
``topos`` binary (v0.5.0+ four-pillar medals). Requires ``gitnexus analyze``
 beforehand for COMPOSABLE / PLATINUM scoring.

Example::

    gitnexus analyze --skip-agents-md
    cargo build --release -p topos
    python scripts/generate_self_badge.py
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import statistics
import subprocess
import sys
from collections import Counter, defaultdict
from pathlib import Path
from xml.sax.saxutils import escape

ROOT = Path(__file__).resolve().parent.parent
DEFAULT_PATHS = ("topos/cli", "topos/engine", "topos/mcp")
DEFAULT_OUT = ROOT / "docs" / "badge.svg"
PILLARS = ("SIMPLE", "COMPOSABLE", "SECURE", "NAVIGABLE")
SCORE_KEYS = ("simple", "secure", "composable", "navigable")
MEDAL_BY_COUNT = {4: "PLATINUM", 3: "GOLD", 2: "SILVER", 1: "BRONZE", 0: "SLOP"}
MEDAL_TIER = {
    "PLATINUM": "gold",
    "GOLD": "gold",
    "SILVER": "silver",
    "BRONZE": "bronze",
    "SLOP": "slop",
}
MEDAL_RANK = {"PLATINUM": 5, "GOLD": 4, "SILVER": 3, "BRONZE": 2, "SLOP": 1}

# Mirrored from topos-leaderboard/leaderboard/badges.py — keep medal colors in sync.
TIER_COLORS = {
    "gold": "#b45309",
    "silver": "#525252",
    "bronze": "#92400e",
    "slop": "#a3a3a3",
}
LABEL_TEXT = "topos"
LABEL_COLOR = "#ffffff"
LABEL_TEXT_COLOR = "#141414"
BADGE_FONT = "Verdana,Geneva,DejaVu Sans,sans-serif"
_CHAR_WIDTH = 6.2
_PADDING = 10.0


def resolve_topos_bin() -> str:
    if env := os.environ.get("TOPOS_BIN"):
        return env
    for candidate in (
        ROOT / "target" / "release" / "topos",
        ROOT / "target" / "debug" / "topos",
    ):
        if candidate.is_file():
            return str(candidate)
    found = shutil.which("topos")
    if found:
        return found
    sys.exit(
        "topos binary not found — set TOPOS_BIN, build with "
        "`cargo build --release -p topos`, or install topos on PATH"
    )


def run_evaluate(topos_bin: str, paths: tuple[str, ...]) -> dict:
    cmd = [topos_bin, "evaluate", *paths, "-r", "--json"]
    completed = subprocess.run(
        cmd,
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
    )
    return json.loads(completed.stdout)


def file_medal(record: dict) -> str:
    dims = record.get("dimensions") or {}
    passed = sum(1 for pillar in PILLARS if dims.get(pillar.lower()) == pillar)
    return MEDAL_BY_COUNT[passed]


def aggregate(records: list[dict]) -> dict:
    if not records:
        sys.exit("evaluate returned no file results")

    file_medals = Counter(file_medal(r) for r in records)
    package_medal = max(file_medals.items(), key=lambda x: (x[1], MEDAL_RANK[x[0]]))[0]

    scores_by_key: dict[str, list[float]] = defaultdict(list)
    for record in records:
        scores = record.get("scores") or {}
        for key in SCORE_KEYS:
            val = scores.get(key)
            if isinstance(val, (int, float)):
                scores_by_key[key].append(float(val))

    mean_scores = {
        key: round(statistics.mean(vals), 1)
        for key, vals in scores_by_key.items()
        if vals
    }
    if not mean_scores:
        sys.exit("evaluate returned no pillar scores")
    composite = round(sum(mean_scores.values()) / len(mean_scores), 1)

    return {
        "medal": package_medal,
        "medal_tier": MEDAL_TIER[package_medal],
        "composite_score": composite,
        "mean_scores": mean_scores,
        "n_files": len(records),
        "file_medals": dict(file_medals),
    }


def _segment_width(text: str) -> int:
    return round(_CHAR_WIDTH * len(text) + _PADDING)


def render_badge(message: str, *, tier: str) -> str:
    color = TIER_COLORS.get(tier, TIER_COLORS["slop"])
    label = escape(LABEL_TEXT)
    msg = escape(message)
    left_w = _segment_width(LABEL_TEXT)
    right_w = _segment_width(message)
    total = left_w + right_w
    label_x = left_w / 2
    return f"""<svg xmlns="http://www.w3.org/2000/svg" width="{total}" height="20" role="img" aria-label="{label}: {msg}">
  <linearGradient id="s" x2="0" y2="100%">
    <stop offset="0" stop-color="#bbb" stop-opacity=".1"/>
    <stop offset="1" stop-opacity=".1"/>
  </linearGradient>
  <clipPath id="r"><rect width="{total}" height="20" rx="3" fill="#fff"/></clipPath>
  <g clip-path="url(#r)">
    <rect width="{left_w}" height="20" fill="{LABEL_COLOR}"/>
    <rect x="{left_w}" width="{right_w}" height="20" fill="{color}"/>
    <rect x="{left_w}" width="{right_w}" height="20" fill="url(#s)"/>
  </g>
  <rect x=".5" y=".5" width="{total - 1}" height="19" rx="2.5" fill="none" stroke="{color}" stroke-width="1"/>
  <g text-anchor="middle" font-family="{BADGE_FONT}" font-size="11">
    <text x="{label_x:g}" y="14" fill="{LABEL_TEXT_COLOR}">{label}</text>
    <text x="{left_w + right_w / 2:g}" y="14" fill="#fff">{msg}</text>
  </g>
</svg>
"""


def badge_message(summary: dict) -> str:
    label = str(summary["medal"])
    score = summary["composite_score"]
    return f"{label} · {score}"


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Evaluate Topos core crates and write docs/badge.svg."
    )
    parser.add_argument(
        "--eval-json",
        type=Path,
        help="Skip evaluate; read existing `topos evaluate --json` output.",
    )
    parser.add_argument(
        "--topos-bin",
        default=None,
        help="Topos CLI to invoke (default: TOPOS_BIN, target/release/topos, PATH).",
    )
    parser.add_argument(
        "--path",
        action="append",
        dest="paths",
        metavar="DIR",
        help=f"Source tree to score (default: {', '.join(DEFAULT_PATHS)}).",
    )
    parser.add_argument(
        "-o",
        "--out",
        type=Path,
        default=DEFAULT_OUT,
        help=f"Output SVG path (default: {DEFAULT_OUT.relative_to(ROOT)}).",
    )
    parser.add_argument(
        "--summary-json",
        action="store_true",
        help="Print aggregation JSON to stdout after writing the badge.",
    )
    args = parser.parse_args()

    paths = tuple(args.paths or DEFAULT_PATHS)
    if args.eval_json:
        payload = json.loads(args.eval_json.read_text(encoding="utf-8"))
    else:
        topos_bin = args.topos_bin or resolve_topos_bin()
        payload = run_evaluate(topos_bin, paths)

    records = payload.get("results") or []
    summary = aggregate(records)
    svg = render_badge(badge_message(summary), tier=summary["medal_tier"])

    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(svg, encoding="utf-8")

    version = payload.get("version", "?")
    print(
        f"Wrote {args.out} — {summary['medal']} · {summary['composite_score']} "
        f"({summary['n_files']} files, topos {version})"
    )
    if args.summary_json:
        json.dump(summary, sys.stdout, indent=2)
        sys.stdout.write("\n")


if __name__ == "__main__":
    main()
