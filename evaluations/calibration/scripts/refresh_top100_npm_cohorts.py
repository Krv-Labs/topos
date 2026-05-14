#!/usr/bin/env python3
"""Regenerate npm cohort lists using npms.io plus npm registry metadata."""

from __future__ import annotations

import json
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[3]
CAL = REPO_ROOT / "evaluations" / "calibration"
OUT_JS = CAL / "top100_npm_js.txt"
OUT_TS = CAL / "top100_npm_ts.txt"
USER_AGENT = "topos-calibration (https://github.com/krv-ai/topos; contact@krv.ai)"


def npms_search(size: int = 250) -> list[str]:
    q = urllib.parse.quote("not:deprecated")
    url = f"https://api.npms.io/v2/search?q={q}&size={size}"
    req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    with urllib.request.urlopen(req, timeout=60) as resp:
        data = json.loads(resp.read().decode("utf-8"))
    return [r["package"]["name"] for r in data.get("results", [])]


def npm_has_typescript_signal(package: str) -> bool:
    if package.startswith("@types/"):
        return True
    enc = urllib.parse.quote(package, safe="")
    url = f"https://registry.npmjs.org/{enc}"
    req = urllib.request.Request(
        url,
        headers={
            "User-Agent": USER_AGENT,
            "Accept": "application/vnd.npm.install-v1+json",
        },
    )
    with urllib.request.urlopen(req, timeout=30) as resp:
        meta = json.loads(resp.read().decode("utf-8"))
    ver = meta["dist-tags"]["latest"]
    man = meta["versions"][ver]
    dev = man.get("devDependencies") or {}
    prod = man.get("dependencies") or {}
    return bool(
        man.get("types")
        or man.get("typings")
        or "typescript" in dev
        or "typescript" in prod
    )


def main() -> None:
    names = npms_search(400)
    ts_pkgs: list[str] = []
    js_pkgs: list[str] = []
    for name in names:
        if len(ts_pkgs) >= 100 and len(js_pkgs) >= 100:
            break
        try:
            is_ts = npm_has_typescript_signal(name)
        except (
            urllib.error.HTTPError,
            urllib.error.URLError,
            KeyError,
            json.JSONDecodeError,
        ):
            continue
        if is_ts and len(ts_pkgs) < 100 and name not in ts_pkgs:
            ts_pkgs.append(name)
        if not is_ts and len(js_pkgs) < 100 and name not in js_pkgs:
            js_pkgs.append(name)

    hdr_js = (
        "# JS-primary npm (npms.io + typings heuristic; auto-generated).\n"
        "# Regenerate: refresh_top100_npm_cohorts.py\n"
    )
    hdr_ts = (
        "# TS-primary npm (npms.io + typings heuristic; auto-generated).\n"
        "# Regenerate: refresh_top100_npm_cohorts.py\n"
    )
    OUT_JS.write_text(hdr_js + "\n".join(js_pkgs) + "\n", encoding="utf-8")
    OUT_TS.write_text(hdr_ts + "\n".join(ts_pkgs) + "\n", encoding="utf-8")
    print(f"Wrote {len(js_pkgs)} JS and {len(ts_pkgs)} TS packages")


if __name__ == "__main__":
    main()
