#!/usr/bin/env python3
"""Write ``top100_crates_io.txt`` from crates.io sorted-by-downloads listing."""

from __future__ import annotations

import json
import urllib.request
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[3]
OUT = REPO_ROOT / "benchmarks" / "calibration" / "top100_crates_io.txt"
USER_AGENT = "topos-calibration (https://github.com/krv-ai/topos; contact@krv.ai)"


def main() -> None:
    url = "https://api.crates.io/api/v1/crates?sort=downloads&per_page=100"
    req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    with urllib.request.urlopen(req, timeout=60) as resp:
        data = json.loads(resp.read().decode("utf-8"))
    names = [c["id"] for c in data["crates"]]
    header = (
        "# Top-100 Rust crates by crates.io downloads (auto-generated).\n"
        "# Regenerate: refresh_top100_crates_io.py\n"
    )
    OUT.write_text(header + "\n".join(names) + "\n", encoding="utf-8")
    print(f"Wrote {len(names)} crates to {OUT}")


if __name__ == "__main__":
    main()
