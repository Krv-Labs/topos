#!/usr/bin/env python3
"""Evaluate the Topos repo and write README badge SVGs.

Writes ``docs/source/_static/topos-lattice-badge.svg`` (four-pillar shields row) and
``docs/source/_static/topos-verdict.svg`` (compact T-logo | medal footer tag).

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
from functools import lru_cache
import sys
from collections import Counter, defaultdict
from pathlib import Path
from xml.etree import ElementTree as ET
from xml.sax.saxutils import escape

ROOT = Path(__file__).resolve().parent.parent
DEFAULT_PATHS = ("topos/cli", "topos/engine", "topos/mcp")
DEFAULT_OUT = ROOT / "docs" / "source" / "_static" / "topos-lattice-badge.svg"
DEFAULT_FOOTER_OUT = ROOT / "docs" / "source" / "_static" / "topos-verdict.svg"
FOOTER_HEIGHT = 24
FOOTER_ICON_SIZE = 14
FOOTER_ICON_X = 6
FOOTER_ICON_Y = 5
FOOTER_MEDAL_PANEL_W = 28  # single medal emoji
FOOTER_EMOJI_FONT = "Apple Color Emoji, Segoe UI Emoji, Noto Color Emoji, sans-serif"
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
# Mirrored from topos/engine/src/core/omega.rs — medal count → lattice symbol.
MEDAL_EMOJI = {
    "PLATINUM": "🏆",
    "GOLD": "🥇",
    "SILVER": "🥈",
    "BRONZE": "🥉",
    "SLOP": "❌",
}

# Mirrored from topos-leaderboard/leaderboard/badges.py — keep medal colors in sync.
TIER_COLORS = {
    "gold": "#b45309",
    "silver": "#525252",
    "bronze": "#92400e",
    "slop": "#a3a3a3",
}
LABEL_TEXT = "topos"
LABEL_COLOR = "#ffffff"
BADGE_FONT = "Verdana,Geneva,DejaVu Sans,sans-serif"

PILLAR_ABBR = {
    "simple": "Si",
    "composable": "Co",
    "secure": "Se",
    "navigable": "Na",
}
FAIL_COLOR = "#78716c"  # warm stone gray — a failed pillar drops out of the medal color
CHIP_WIDTH = 28.0

ICON_PATH = ROOT / "docs" / "topos-icon.svg"
ICON_SIZE = 16
ICON_X = 5
ICON_Y = 2
_SKIP_ICON_TAGS = {"metadata", "namedview", "title"}


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

    # A pillar passes for the package when most files pass it — the same
    # majority rule behind the package medal.
    pillar_pass = {
        pillar.lower(): sum(
            1
            for r in records
            if (r.get("dimensions") or {}).get(pillar.lower()) == pillar
        )
        * 2
        > len(records)
        for pillar in PILLARS
    }

    return {
        "medal": package_medal,
        "medal_tier": MEDAL_TIER[package_medal],
        "composite_score": composite,
        "mean_scores": mean_scores,
        "pillar_pass": pillar_pass,
        "n_files": len(records),
        "file_medals": dict(file_medals),
    }


def _local_svg_name(tag: str) -> str:
    return tag.rsplit("}", 1)[-1]


def _strip_svg_namespace(el: ET.Element) -> None:
    el.tag = _local_svg_name(el.tag)
    el.attrib = {
        key: val for key, val in el.attrib.items() if not key.startswith("{")
    }
    for child in list(el):
        _strip_svg_namespace(child)


def _icon_markup(*, size: int, x: int, y: int) -> str:
    if not ICON_PATH.is_file():
        return ""

    root = ET.parse(ICON_PATH).getroot()
    view_box = root.attrib.get("viewBox", "0 0 240 240")
    _strip_svg_namespace(root)

    children = []
    for child in list(root):
        if _local_svg_name(child.tag) in _SKIP_ICON_TAGS:
            continue
        children.append(
            ET.tostring(child, encoding="unicode", short_empty_elements=True)
        )
    if not children:
        return ""

    return (
        f'<svg x="{x}" y="{y}" width="{size}" height="{size}" '
        f'viewBox="{escape(view_box)}" aria-hidden="true" focusable="false" '
        'preserveAspectRatio="xMidYMid meet">'
        + "".join(children)
        + "</svg>"
    )


@lru_cache(maxsize=1)
def _render_icon_svg() -> str:
    return _icon_markup(size=ICON_SIZE, x=ICON_X, y=ICON_Y)


@lru_cache(maxsize=1)
def _render_footer_icon_svg() -> str:
    return _icon_markup(
        size=FOOTER_ICON_SIZE, x=FOOTER_ICON_X, y=FOOTER_ICON_Y
    )


def render_badge(pillars: list[tuple[str, bool]], *, tier: str) -> str:
    """Render `[icon][Si][Co][Se][Na]` — one 20px shields-style row.

    Each pillar chip carries the medal color when that pillar passes for the
    package and a muted gray when it does not, so the chip count reads as the
    medal: four colored chips is PLATINUM, three is GOLD, and so on.
    """
    color = TIER_COLORS.get(tier, TIER_COLORS["slop"])
    icon = _render_icon_svg()
    plate_w = ICON_X + ICON_SIZE + ICON_X if icon else 0
    total = round(plate_w + CHIP_WIDTH * len(pillars))

    chips, labels, seps = [], [], []
    for i, (abbr, passed) in enumerate(pillars):
        x = plate_w + CHIP_WIDTH * i
        chips.append(
            f'<rect x="{x:g}" width="{CHIP_WIDTH:g}" height="20" '
            f'fill="{color if passed else FAIL_COLOR}"/>'
        )
        labels.append(
            f'<text x="{x + CHIP_WIDTH / 2:g}" y="14" fill="#fff" '
            f'fill-opacity="{1 if passed else 0.85}">{escape(abbr)}</text>'
        )
        if i:
            seps.append(
                f'<rect x="{x:g}" y="3" width="1" height="14" fill="#000" fill-opacity=".18"/>'
            )
    aria = ", ".join(f"{abbr} {'pass' if ok else 'fail'}" for abbr, ok in pillars)

    return f"""<svg xmlns="http://www.w3.org/2000/svg" width="{total}" height="20" role="img" aria-label="{LABEL_TEXT}: {escape(aria)}">
  <linearGradient id="s" x2="0" y2="100%">
    <stop offset="0" stop-color="#bbb" stop-opacity=".1"/>
    <stop offset="1" stop-opacity=".1"/>
  </linearGradient>
  <clipPath id="r"><rect width="{total}" height="20" rx="3" fill="#fff"/></clipPath>
  <g clip-path="url(#r)">
    <rect width="{plate_w:g}" height="20" fill="{LABEL_COLOR}"/>
    {"".join(chips)}
    {"".join(seps)}
    <rect x="{plate_w:g}" width="{total - plate_w:g}" height="20" fill="url(#s)"/>
  </g>
  <rect x=".5" y=".5" width="{total - 1}" height="19" rx="2.5" fill="none" stroke="{color}" stroke-width="1"/>
  {icon}
  <g text-anchor="middle" font-family="{BADGE_FONT}" font-size="11" font-weight="bold">
    {"".join(labels)}
  </g>
</svg>
"""


def render_footer_badge(medal: str, *, tier: str) -> str:
    """Render `[icon][emoji]` — a compact 24px two-panel footer tag."""
    color = TIER_COLORS.get(tier, TIER_COLORS["slop"])
    emoji = MEDAL_EMOJI.get(medal, MEDAL_EMOJI["SLOP"])
    icon = _render_footer_icon_svg()
    plate_w = FOOTER_ICON_X + FOOTER_ICON_SIZE + FOOTER_ICON_X if icon else 0
    total = round(plate_w + FOOTER_MEDAL_PANEL_W)
    medal_x = plate_w + FOOTER_MEDAL_PANEL_W / 2
    aria = f"Topos lattice verdict: {medal}"

    return f"""<svg xmlns="http://www.w3.org/2000/svg" width="{total}" height="{FOOTER_HEIGHT}" role="img" aria-label="{escape(aria)}">
  <linearGradient id="s" x2="0" y2="100%">
    <stop offset="0" stop-color="#bbb" stop-opacity=".1"/>
    <stop offset="1" stop-opacity=".1"/>
  </linearGradient>
  <clipPath id="r"><rect width="{total}" height="{FOOTER_HEIGHT}" rx="3.2" fill="#fff"/></clipPath>
  <g clip-path="url(#r)">
    <rect width="{plate_w:g}" height="{FOOTER_HEIGHT}" fill="{LABEL_COLOR}"/>
    <rect x="{plate_w:g}" width="{FOOTER_MEDAL_PANEL_W:g}" height="{FOOTER_HEIGHT}" fill="{color}"/>
    <rect x="{plate_w:g}" y="4" width="1" height="16" fill="#000" fill-opacity=".18"/>
    <rect x="{plate_w:g}" width="{FOOTER_MEDAL_PANEL_W:g}" height="{FOOTER_HEIGHT}" fill="url(#s)"/>
  </g>
  <rect x=".5" y=".5" width="{total - 1}" height="{FOOTER_HEIGHT - 1}" rx="3.2" fill="none" stroke="{color}" stroke-width="1"/>
  {icon}
  <text x="{medal_x:g}" y="17" text-anchor="middle" font-family="{FOOTER_EMOJI_FONT}" font-size="14">{emoji}</text>
</svg>
"""


def badge_pillars(summary: dict) -> list[tuple[str, bool]]:
    passed = summary.get("pillar_pass") or {}
    return [
        (PILLAR_ABBR[key], bool(passed.get(key)))
        for key in (pillar.lower() for pillar in PILLARS)
    ]


def self_check() -> None:
    """Assert the rendered badge is well-formed SVG with consistent geometry."""
    pillars = [("Si", True), ("Co", False), ("Se", True), ("Na", True)]
    for tier in TIER_COLORS:
        svg = render_badge(pillars, tier=tier)
        root = ET.fromstring(svg)
        total = float(root.attrib["width"])
        rects = [
            r
            for r in root.iter("{http://www.w3.org/2000/svg}rect")
            if r.attrib.get("height") == "20"
        ]
        assert rects, "no segment rects"
        right = max(float(r.attrib.get("x", 0)) + float(r.attrib["width"]) for r in rects)
        assert abs(right - total) < 1, f"{tier}: segments {right} != width {total}"
    assert 'fill="#78716c"' in render_badge(pillars, tier="gold"), "failed chip not muted"
    for medal, tier in (
        ("PLATINUM", "gold"),
        ("GOLD", "gold"),
        ("SILVER", "silver"),
        ("BRONZE", "bronze"),
        ("SLOP", "slop"),
    ):
        footer = render_footer_badge(medal, tier=tier)
        root = ET.fromstring(footer)
        assert float(root.attrib["height"]) == FOOTER_HEIGHT
        assert MEDAL_EMOJI[medal] in footer
    print("self-check ok")


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Evaluate Topos core crates and write README badge SVGs."
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
        "--footer-out",
        type=Path,
        default=DEFAULT_FOOTER_OUT,
        help=(
            "Compact footer badge SVG path "
            f"(default: {DEFAULT_FOOTER_OUT.relative_to(ROOT)})."
        ),
    )
    parser.add_argument(
        "--self-check",
        action="store_true",
        help="Render sample badges, assert geometry, and exit.",
    )
    parser.add_argument(
        "--summary-json",
        action="store_true",
        help="Print aggregation JSON to stdout after writing the badge.",
    )
    args = parser.parse_args()
    if args.self_check:
        self_check()
        return

    paths = tuple(args.paths or DEFAULT_PATHS)
    if args.eval_json:
        payload = json.loads(args.eval_json.read_text(encoding="utf-8"))
    else:
        topos_bin = args.topos_bin or resolve_topos_bin()
        payload = run_evaluate(topos_bin, paths)

    records = payload.get("results") or []
    summary = aggregate(records)
    svg = render_badge(badge_pillars(summary), tier=summary["medal_tier"])
    footer_svg = render_footer_badge(summary["medal"], tier=summary["medal_tier"])

    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(svg, encoding="utf-8")
    args.footer_out.parent.mkdir(parents=True, exist_ok=True)
    args.footer_out.write_text(footer_svg, encoding="utf-8")

    version = payload.get("version", "?")
    status = (
        f"Wrote {args.out} and {args.footer_out} — {summary['medal']} · "
        f"{summary['composite_score']} ({summary['n_files']} files, topos {version})"
    )
    if args.summary_json:
        print(status, file=sys.stderr)
        json.dump(summary, sys.stdout, indent=2)
        sys.stdout.write("\n")
    else:
        print(status)


if __name__ == "__main__":
    main()
