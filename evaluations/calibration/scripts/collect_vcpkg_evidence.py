"""Read ``vcpkg.json`` for each port under ``$VCPKG_ROOT/ports``.

Requires ``VCPKG_ROOT`` and ``python collect_vcpkg_evidence.py``.
"""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[3]
CALIBRATION_DIR = REPO_ROOT / "evaluations" / "calibration"


def derive_signals(manifest: dict) -> dict:
    name = manifest.get("name", "")
    desc = str(manifest.get("description") or "")[:200]
    deps = manifest.get("dependencies")
    direct_dep_count = len(deps) if isinstance(deps, (list, dict)) else 0

    features = manifest.get("features") or {}
    if isinstance(features, dict) and len(features) > 25:
        sig, conf = "self_contained", "medium"
    elif direct_dep_count > 12:
        sig, conf = "composable", "medium"
    else:
        sig, conf = "composable", "low"

    return {
        "port_name": name,
        "description": desc,
        "signal_classification": sig,
        "signal_confidence": conf,
        "direct_dep_count": direct_dep_count,
    }


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Collect vcpkg port metadata from a local tree."
    )
    parser.add_argument("--port", action="append", dest="ports")
    parser.add_argument(
        "--cohort",
        type=Path,
        default=CALIBRATION_DIR / "top100_vcpkg_ports.txt",
    )
    parser.add_argument("--vcpkg-root", type=Path, default=None)
    parser.add_argument(
        "--output",
        type=Path,
        default=CALIBRATION_DIR / "evidence" / "vcpkg_evidence.jsonl",
    )
    args = parser.parse_args()

    root = args.vcpkg_root or os.environ.get("VCPKG_ROOT")
    if not root:
        parser.error("--vcpkg-root or VCPKG_ROOT is required")
    vcpkg_root = Path(root).resolve()

    if args.ports:
        names = args.ports
    else:
        if not args.cohort.is_file():
            parser.error(f"Cohort file not found: {args.cohort}")
        names = [
            line.strip()
            for line in args.cohort.read_text(encoding="utf-8").splitlines()
            if line.strip() and not line.startswith("#")
        ]

    args.output.parent.mkdir(parents=True, exist_ok=True)
    print(f"Reading {len(names)} ports from {vcpkg_root / 'ports'} → {args.output}")
    with args.output.open("w", encoding="utf-8") as out:
        for name in names:
            pj = vcpkg_root / "ports" / name / "vcpkg.json"
            if not pj.is_file():
                out.write(
                    json.dumps({"package": name, "error": "missing_vcpkg_json"}) + "\n"
                )
                print(f"  [warn] missing {pj}")
                continue
            manifest = json.loads(pj.read_text(encoding="utf-8"))
            sig = derive_signals(manifest)
            out.write(json.dumps({"package": name, **sig}) + "\n")
            print(
                f"{name}: {sig['signal_classification']} ({sig['signal_confidence']})"
            )
    print("Done.")


if __name__ == "__main__":
    main()
