"""Collect npm registry metadata for a cohort (JS or TS list).

Writes JSONL compatible with ``analyze_scores.py``. Run with ``--cohort``;
default is ``top100_npm_js.txt``.
"""

from __future__ import annotations

import argparse
import json
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[3]
CALIBRATION_DIR = REPO_ROOT / "benchmarks" / "calibration"
USER_AGENT = "topos-calibration (https://github.com/krv-ai/topos; contact@krv.ai)"


def fetch_manifest(package: str) -> dict | None:
    enc = urllib.parse.quote(package, safe="")
    url = f"https://registry.npmjs.org/{enc}"
    req = urllib.request.Request(
        url,
        headers={
            "User-Agent": USER_AGENT,
            "Accept": "application/vnd.npm.install-v1+json",
        },
    )
    try:
        with urllib.request.urlopen(req, timeout=30) as resp:
            return json.loads(resp.read().decode("utf-8"))
    except (
        urllib.error.HTTPError,
        urllib.error.URLError,
        json.JSONDecodeError,
        OSError,
    ) as exc:
        print(f"  [warn] fetch failed for {package}: {exc}")
        return None


def derive_signals(meta: dict) -> dict:
    ver = meta["dist-tags"]["latest"]
    man = meta["versions"][ver]
    desc = (man.get("description") or "")[:200]
    keywords = man.get("keywords") or []
    if isinstance(keywords, str):
        keywords = [keywords]
    has_types = bool(man.get("types") or man.get("typings"))
    deps = man.get("dependencies") or {}
    direct_dep_count = len(deps)
    blob = " ".join(keywords).lower() + " " + desc.lower()

    if any(
        k in blob
        for k in ("cli", "command", "server", "framework", "express", "webpack")
    ):
        sig, conf = "self_contained", "medium"
    elif any(k in blob for k in ("util", "lib", "parser", "sdk", "client", "driver")):
        sig, conf = "composable", "medium"
    else:
        sig, conf = "composable", "low"

    return {
        "description": desc,
        "keywords": keywords[:40],
        "has_published_types": has_types,
        "signal_classification": sig,
        "signal_confidence": conf,
        "direct_dep_count": direct_dep_count,
    }


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Collect npm registry metadata for calibration."
    )
    parser.add_argument("--package", action="append", dest="packages")
    parser.add_argument(
        "--cohort",
        type=Path,
        default=CALIBRATION_DIR / "top100_npm_js.txt",
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=CALIBRATION_DIR / "evidence" / "npm_evidence.jsonl",
    )
    parser.add_argument("--delay", type=float, default=0.25)
    args = parser.parse_args()

    if args.packages:
        names = args.packages
    else:
        if not args.cohort.is_file():
            parser.error(f"Cohort file not found: {args.cohort}")
        names = [
            line.strip()
            for line in args.cohort.read_text(encoding="utf-8").splitlines()
            if line.strip() and not line.startswith("#")
        ]

    args.output.parent.mkdir(parents=True, exist_ok=True)
    print(f"Processing {len(names)} packages → {args.output}")
    with args.output.open("w", encoding="utf-8") as out:
        for i, name in enumerate(names, start=1):
            print(f"[{i}/{len(names)}] {name} ...", end=" ", flush=True)
            meta = fetch_manifest(name)
            if not meta:
                out.write(json.dumps({"package": name, "error": "fetch_failed"}) + "\n")
                print("ERROR")
            else:
                sig = derive_signals(meta)
                out.write(json.dumps({"package": name, **sig}) + "\n")
                print(f"{sig['signal_classification']} ({sig['signal_confidence']})")
            if i < len(names):
                time.sleep(args.delay)
    print("Done.")


if __name__ == "__main__":
    main()
