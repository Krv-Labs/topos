"""Cross-validate manual usage labels against automated signals from PyPI metadata.

For each package in the cohort, fetches the PyPI JSON API and derives a
``signal_classification`` (composable | self_contained) and
``signal_confidence`` (high | medium | low) from Trove classifiers and
dependency counts.  Writes one JSONL record per package and prints any
disagreements with the manual labels in ``usage_profiles.csv``.

Run:
    python evaluations/calibration/scripts/collect_pypi_evidence.py
    python evaluations/calibration/scripts/collect_pypi_evidence.py \\
        --package requests --package httpx
"""

from __future__ import annotations

import argparse
import csv
import json
import time
import urllib.error
import urllib.request
from pathlib import Path

# ---------------------------------------------------------------------------
# Trove classifier prefix groups
# ---------------------------------------------------------------------------

_FRAMEWORK_PREFIXES: tuple[str, ...] = (
    "Framework ::",
    "Framework::",
    "Environment :: Web Environment",
    "Environment :: Console",
    "Topic :: Internet :: WWW/HTTP :: WSGI",
    "Topic :: Internet :: WWW/HTTP :: Dynamic Content",
    "Topic :: Software Development :: Libraries :: Application Frameworks",
)

_LIBRARY_PREFIXES: tuple[str, ...] = (
    "Topic :: Software Development :: Libraries :: Python Modules",
    "Topic :: Software Development :: Libraries",
    "Intended Audience :: Developers",
    "Topic :: Utilities",
    "Topic :: Software Development :: Code Generators",
    "Topic :: Software Development :: Compilers",
    "Topic :: Text Processing",
    "Topic :: Scientific/Engineering",
    "Topic :: Software Development :: Testing",
)

_TOOL_PREFIXES: tuple[str, ...] = (
    "Topic :: Software Development :: Build Tools",
    "Topic :: System :: Systems Administration",
    "Topic :: System :: Monitoring",
    "Topic :: System :: Networking",
    "Topic :: System :: Distributed Computing",
    "Environment :: Plugins",
)


# ---------------------------------------------------------------------------
# Signal derivation
# ---------------------------------------------------------------------------


def _has_prefix(classifiers: list[str], prefixes: tuple[str, ...]) -> bool:
    """Return True if any classifier starts with any of the given prefixes."""
    for clf in classifiers:
        for prefix in prefixes:
            if clf.startswith(prefix):
                return True
    return False


def derive_signals(pypi_info: dict) -> dict:
    """Derive classification signals from a PyPI package info dict.

    Parameters
    ----------
    pypi_info:
        The ``info`` sub-dict from ``https://pypi.org/pypi/{package}/json``.

    Returns
    -------
    dict with keys:
        has_framework_classifier, has_library_classifier, has_tool_classifier,
        direct_dep_count, requires_python, summary,
        signal_classification, signal_confidence, pypi_classifiers.

    Classification logic
    --------------------
    - framework AND NOT library  → self_contained, high
    - library AND NOT framework  → composable, high
    - tool AND NOT library AND NOT framework  → self_contained, medium
    - else                       → composable, low  (inconclusive)
    """
    classifiers: list[str] = pypi_info.get("classifiers") or []

    has_framework = _has_prefix(classifiers, _FRAMEWORK_PREFIXES)
    has_library = _has_prefix(classifiers, _LIBRARY_PREFIXES)
    has_tool = _has_prefix(classifiers, _TOOL_PREFIXES)

    # Count non-extra requires_dist entries as a rough dependency signal.
    requires_dist: list[str] | None = pypi_info.get("requires_dist")
    if requires_dist:
        direct_dep_count = sum(
            1 for dep in requires_dist if dep and "extra ==" not in dep.lower()
        )
    else:
        direct_dep_count = 0

    summary_raw: str = pypi_info.get("summary") or ""
    summary = summary_raw[:120]

    # Determine signal classification and confidence.
    if has_framework and not has_library:
        signal_classification = "self_contained"
        signal_confidence = "high"
    elif has_library and not has_framework:
        signal_classification = "composable"
        signal_confidence = "high"
    elif has_tool and not has_library and not has_framework:
        signal_classification = "self_contained"
        signal_confidence = "medium"
    else:
        signal_classification = "composable"
        signal_confidence = "low"

    return {
        "has_framework_classifier": has_framework,
        "has_library_classifier": has_library,
        "has_tool_classifier": has_tool,
        "direct_dep_count": direct_dep_count,
        "requires_python": pypi_info.get("requires_python") or "",
        "summary": summary,
        "signal_classification": signal_classification,
        "signal_confidence": signal_confidence,
        "pypi_classifiers": classifiers,
    }


# ---------------------------------------------------------------------------
# PyPI fetch
# ---------------------------------------------------------------------------


def fetch_pypi(package: str) -> dict | None:
    """Fetch package info from the PyPI JSON API.

    Returns the ``info`` sub-dict, or None on any error.
    """
    url = f"https://pypi.org/pypi/{package}/json"
    try:
        with urllib.request.urlopen(url, timeout=15) as response:
            data = json.loads(response.read().decode("utf-8"))
        return data.get("info") or {}
    except urllib.error.HTTPError as exc:
        print(f"  [warn] HTTP {exc.code} fetching {package}: {exc.reason}")
        return None
    except urllib.error.URLError as exc:
        print(f"  [warn] URL error fetching {package}: {exc.reason}")
        return None
    except Exception as exc:  # noqa: BLE001
        print(f"  [warn] Unexpected error fetching {package}: {exc}")
        return None


# ---------------------------------------------------------------------------
# CSV helpers
# ---------------------------------------------------------------------------


def _load_usage_profiles(profiles_path: Path) -> dict[str, str]:
    """Load package → usage_classification mapping from CSV.

    Expects columns: ``package``, ``usage_classification``.
    Returns empty dict if file not found.
    """
    if not profiles_path.is_file():
        return {}
    mapping: dict[str, str] = {}
    with profiles_path.open(newline="", encoding="utf-8") as fh:
        reader = csv.DictReader(fh)
        for row in reader:
            pkg = (row.get("package") or "").strip()
            label = (row.get("usage_classification") or "").strip()
            if pkg:
                mapping[pkg] = label
    return mapping


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------


def _build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description=(
            "Cross-validate manual usage labels against PyPI metadata signals. "
            "Writes one JSONL record per package and prints disagreements."
        )
    )
    parser.add_argument(
        "--package",
        action="append",
        dest="packages",
        metavar="PACKAGE",
        help="Process a single package (may be repeated). Skips the cohort file.",
    )
    parser.add_argument(
        "--cohort",
        type=Path,
        default=Path("evaluations/calibration/top100_pypi.txt"),
        help="Path to the package list (one name per line). "
        "Default: evaluations/calibration/top100_pypi.txt",
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=Path("evaluations/calibration/evidence/pypi_evidence.jsonl"),
        help="Output JSONL path. "
        "Default: evaluations/calibration/evidence/pypi_evidence.jsonl",
    )
    parser.add_argument(
        "--delay",
        type=float,
        default=0.4,
        help="Seconds to sleep between PyPI API requests. Default: 0.4",
    )
    return parser


def main() -> None:
    """Entry point for collect_pypi_evidence."""
    parser = _build_parser()
    args = parser.parse_args()

    # Determine package list.
    if args.packages:
        packages: list[str] = args.packages
    else:
        cohort_path: Path = args.cohort
        if not cohort_path.is_file():
            parser.error(f"Cohort file not found: {cohort_path}")
        packages = [
            line.strip()
            for line in cohort_path.read_text(encoding="utf-8").splitlines()
            if line.strip() and not line.startswith("#")
        ]

    output_path: Path = args.output
    output_path.parent.mkdir(parents=True, exist_ok=True)

    # Load manual labels for cross-validation.
    profiles_path = args.cohort.parent / "usage_profiles.csv"
    manual_labels = _load_usage_profiles(profiles_path)

    print(f"Processing {len(packages)} packages → {output_path}")
    if manual_labels:
        print(f"Loaded {len(manual_labels)} manual labels from {profiles_path}")
    else:
        print(f"No usage_profiles.csv found at {profiles_path} — skipping cross-validation")

    disagreements: list[dict] = []

    with output_path.open("w", encoding="utf-8") as out_fh:
        for idx, package in enumerate(packages, start=1):
            print(f"[{idx}/{len(packages)}] {package} ...", end=" ", flush=True)
            info = fetch_pypi(package)
            if info is None:
                record: dict = {"package": package, "error": "fetch_failed"}
                out_fh.write(json.dumps(record) + "\n")
                print("ERROR")
                if idx < len(packages):
                    time.sleep(args.delay)
                continue

            signals = derive_signals(info)
            record = {"package": package, **signals}
            out_fh.write(json.dumps(record) + "\n")
            print(
                f"{signals['signal_classification']} "
                f"(confidence={signals['signal_confidence']})"
            )

            # Cross-validate against manual label.
            manual = manual_labels.get(package)
            if (
                manual
                and signals["signal_confidence"] in ("high", "medium")
                and signals["signal_classification"] != manual
            ):
                disagreements.append(
                    {
                        "package": package,
                        "manual_label": manual,
                        "signal_classification": signals["signal_classification"],
                        "signal_confidence": signals["signal_confidence"],
                        "has_framework_classifier": signals["has_framework_classifier"],
                        "has_library_classifier": signals["has_library_classifier"],
                    }
                )

            if idx < len(packages):
                time.sleep(args.delay)

    print()
    print(f"Wrote {len(packages)} records to {output_path}")

    if not manual_labels:
        return

    if not disagreements:
        print("No disagreements found between PyPI signals and manual labels.")
        return

    print(f"\n{'='*60}")
    print(f"DISAGREEMENTS ({len(disagreements)} found):")
    print(f"{'='*60}")
    for d in disagreements:
        print(
            f"  {d['package']:<30} "
            f"manual={d['manual_label']:<16} "
            f"signal={d['signal_classification']:<16} "
            f"confidence={d['signal_confidence']}"
        )
        print(
            f"    framework_clf={d['has_framework_classifier']}  "
            f"library_clf={d['has_library_classifier']}"
        )
    print()
    print(
        "Action: review each disagreement in usage_profiles.csv. "
        "For signal_confidence=high disagreements, update the rationale column "
        "or reclassify the entry before running Experiments 4 and 5."
    )


if __name__ == "__main__":
    main()
