"""Collect lightweight registry metadata for Rust crates (crates.io).

Writes ``evaluations/calibration/evidence/crates_evidence.jsonl`` with fields
compatible with ``analyze_scores.py`` (``signal_classification``,
``signal_confidence``, ``direct_dep_count`` proxy).

Run:
    python evaluations/calibration/scripts/collect_crates_evidence.py
    python evaluations/calibration/scripts/collect_crates_evidence.py --crate serde
"""

from __future__ import annotations

import argparse
import json
import time
import urllib.error
import urllib.request
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[3]
CALIBRATION_DIR = REPO_ROOT / "evaluations" / "calibration"
USER_AGENT = "topos-calibration (https://github.com/krv-ai/topos; contact@krv.ai)"


def fetch_crate(crate: str) -> dict | None:
    url = f"https://crates.io/api/v1/crates/{crate}"
    req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    try:
        with urllib.request.urlopen(req, timeout=30) as resp:
            return json.loads(resp.read().decode("utf-8"))
    except (
        urllib.error.HTTPError,
        urllib.error.URLError,
        json.JSONDecodeError,
        OSError,
    ) as exc:
        print(f"  [warn] fetch failed for {crate}: {exc}")
        return None


def derive_signals(payload: dict) -> dict:
    crate = payload.get("crate") or {}
    cats = payload.get("categories") or []
    slugs: list[str] = []
    for c in cats:
        if isinstance(c, dict) and c.get("slug"):
            slugs.append(str(c["slug"]))
    slug_blob = " ".join(slugs).lower()
    desc = (crate.get("description") or "")[:200]
    downloads = int(crate.get("downloads") or 0)

    if any(
        s in slug_blob
        for s in (
            "command-line",
            "web-programming::http-server",
            "development-tools",
            "wasm",
        )
    ):
        sig, conf = "self_contained", "medium"
    elif any(
        s in slug_blob
        for s in ("parsing", "encoding", "parser", "compression", "science")
    ):
        sig, conf = "composable", "medium"
    else:
        sig, conf = "composable", "low"

    return {
        "description": desc,
        "crate_downloads": downloads,
        "categories": slugs,
        "signal_classification": sig,
        "signal_confidence": conf,
        "direct_dep_count": len(slugs),
    }


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Collect crates.io metadata for calibration."
    )
    parser.add_argument("--crate", action="append", dest="crates", metavar="NAME")
    parser.add_argument(
        "--cohort",
        type=Path,
        default=CALIBRATION_DIR / "top100_crates_io.txt",
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=CALIBRATION_DIR / "evidence" / "crates_evidence.jsonl",
    )
    parser.add_argument("--delay", type=float, default=0.4)
    args = parser.parse_args()

    if args.crates:
        names = args.crates
    else:
        if not args.cohort.is_file():
            parser.error(f"Cohort file not found: {args.cohort}")
        names = [
            line.strip()
            for line in args.cohort.read_text(encoding="utf-8").splitlines()
            if line.strip() and not line.startswith("#")
        ]

    args.output.parent.mkdir(parents=True, exist_ok=True)
    print(f"Processing {len(names)} crates → {args.output}")
    with args.output.open("w", encoding="utf-8") as out:
        for i, name in enumerate(names, start=1):
            print(f"[{i}/{len(names)}] {name} ...", end=" ", flush=True)
            data = fetch_crate(name)
            if not data:
                out.write(json.dumps({"package": name, "error": "fetch_failed"}) + "\n")
                print("ERROR")
            else:
                sig = derive_signals(data)
                out.write(json.dumps({"package": name, **sig}) + "\n")
                print(f"{sig['signal_classification']} ({sig['signal_confidence']})")
            if i < len(names):
                time.sleep(args.delay)
    print("Done.")


if __name__ == "__main__":
    main()
