"""Analyze the structural_scores.jsonl output from run_structural_baseline*.py.

For each package in the JSONL, computes per-package summary statistics,
sweeps thresholds, optionally stratifies by usage label (usage_profiles.csv),
and optionally cross-joins with an evidence JSONL (PyPI, crates.io, npm, vcpkg).

Writes a JSON summary to ``evaluations/calibration/results/score_analysis.json``.

Run:
    python evaluations/calibration/scripts/analyze_scores.py
    python evaluations/calibration/scripts/analyze_scores.py \\
        --scores evaluations/calibration/results/structural_scores_rust.jsonl \\
        --evidence evaluations/calibration/evidence/crates_evidence.jsonl \\
        --ecosystem rust --language rust
"""

from __future__ import annotations

import argparse
import csv
import json
import statistics
from collections import defaultdict
from pathlib import Path
from typing import Any

# ---------------------------------------------------------------------------
# Path constants
# ---------------------------------------------------------------------------

REPO_ROOT = Path(__file__).resolve().parents[3]
CALIBRATION_DIR = REPO_ROOT / "evaluations" / "calibration"
RESULTS_DIR = CALIBRATION_DIR / "results"

THRESHOLD_SWEEP = [0.40, 0.50, 0.55, 0.60, 0.65, 0.70]


# ---------------------------------------------------------------------------
# Data loading
# ---------------------------------------------------------------------------


def load_scores(
    path: Path,
    *,
    ecosystem: str | None = None,
    language: str | None = None,
) -> list[dict]:
    """Load per-file records from a JSONL file, skipping error records."""
    records: list[dict] = []
    if not path.is_file():
        raise FileNotFoundError(f"Scores file not found: {path}")
    with path.open(encoding="utf-8") as fh:
        for lineno, line in enumerate(fh, start=1):
            line = line.strip()
            if not line:
                continue
            try:
                record = json.loads(line)
            except json.JSONDecodeError as exc:
                print(f"  [warn] skipping malformed JSON on line {lineno}: {exc}")
                continue
            if "error" in record:
                continue  # skip error records from run_structural_baseline
            if ecosystem is not None and record.get("ecosystem") != ecosystem:
                continue
            if language is not None and record.get("language") != language:
                continue
            records.append(record)
    return records


def _extract_structural_score(record: dict) -> float | None:
    """Extract the structural score from a file-level topos result record.

    The CLI emits scores as values in [0, 100] inside ``scores.structural``,
    or as a top-level ``structural_score`` float.
    """
    scores = record.get("scores")
    if isinstance(scores, dict):
        val = scores.get("structural")
        if isinstance(val, (int, float)):
            return float(val)
    val = record.get("structural_score")
    if isinstance(val, (int, float)):
        return float(val)
    return None


def load_profiles(path: Path) -> dict[str, str]:
    """Load package → usage_classification from CSV.  Returns empty dict if absent."""
    if not path.is_file():
        return {}
    mapping: dict[str, str] = {}
    with path.open(newline="", encoding="utf-8") as fh:
        reader = csv.DictReader(fh)
        for row in reader:
            pkg = (row.get("package") or "").strip()
            label = (row.get("usage_classification") or "").strip()
            if pkg:
                mapping[pkg] = label
    return mapping


def load_evidence(path: Path) -> dict[str, dict]:
    """Load package → signal dict from evidence JSONL."""
    if not path.is_file():
        return {}
    evidence: dict[str, dict] = {}
    with path.open(encoding="utf-8") as fh:
        for line in fh:
            line = line.strip()
            if not line:
                continue
            try:
                record = json.loads(line)
            except json.JSONDecodeError:
                continue
            pkg = record.get("package")
            if pkg:
                evidence[pkg] = record
    return evidence


# ---------------------------------------------------------------------------
# Per-package statistics
# ---------------------------------------------------------------------------


def compute_package_stats(
    records: list[dict],
) -> dict[str, dict]:
    """Group records by package and compute summary statistics.

    Returns
    -------
    dict mapping package name → stats dict with keys:
        n_files, mean_structural, median_structural, stdev_structural,
        pct_passing_0_6, lattice_counts, scores (list of raw score floats).
    """
    by_package: dict[str, list[float]] = defaultdict(list)
    lattice_by_package: dict[str, dict[str, int]] = defaultdict(dict)

    for record in records:
        package = record.get("package", "unknown")
        score = _extract_structural_score(record)
        if score is not None:
            by_package[package].append(score)

        lattice = record.get("lattice_element") or record.get("summary")
        if lattice:
            counts = lattice_by_package[package]
            counts[lattice] = counts.get(lattice, 0) + 1

    stats: dict[str, dict] = {}
    for package, score_list in by_package.items():
        n = len(score_list)
        mean = statistics.mean(score_list) if n else 0.0
        median = statistics.median(score_list) if n else 0.0
        stdev = statistics.stdev(score_list) if n >= 2 else 0.0
        # Scores from the CLI are in [0, 100]; normalize to [0, 1] for threshold
        # comparison.
        pct_passing = (
            sum(1 for s in score_list if s / 100.0 >= 0.6) / n * 100.0 if n else 0.0
        )
        stats[package] = {
            "n_files": n,
            "mean_structural": mean,
            "median_structural": median,
            "stdev_structural": stdev,
            "pct_passing_0_6": pct_passing,
            "lattice_counts": lattice_by_package.get(package, {}),
            "scores": score_list,
        }

    return stats


# ---------------------------------------------------------------------------
# Display helpers
# ---------------------------------------------------------------------------


def print_package_table(stats: dict[str, dict]) -> None:
    """Print per-package summary sorted by mean structural score (descending)."""
    sorted_packages = sorted(
        stats.items(), key=lambda kv: kv[1]["mean_structural"], reverse=True
    )

    header = f"{'package':<35} {'n_files':>7} {'mean':>7} {'median':>7} {'pass@0.6':>9}"
    print(header)
    print("-" * len(header))
    for package, s in sorted_packages:
        print(
            f"{package:<35} "
            f"{s['n_files']:>7} "
            f"{s['mean_structural']:>7.1f} "
            f"{s['median_structural']:>7.1f} "
            f"{s['pct_passing_0_6']:>8.1f}%"
        )


def print_threshold_sweep(stats: dict[str, dict]) -> None:
    """Print how many packages and files pass at each threshold."""
    # Collect all file scores.
    all_scores: list[float] = []
    for s in stats.values():
        all_scores.extend(s["scores"])

    total_files = len(all_scores)
    total_packages = len(stats)

    print(f"\nThreshold sweep ({total_packages} packages, {total_files} files):")
    print(f"  {'threshold':>9}  {'pkg_pass':>9}  {'file_pass':>9}")
    print("  " + "-" * 32)
    for threshold in THRESHOLD_SWEEP:
        pkg_pass = sum(
            1
            for s in stats.values()
            if s["scores"] and statistics.mean(s["scores"]) / 100.0 >= threshold
        )
        file_pass = sum(1 for score in all_scores if score / 100.0 >= threshold)
        pkg_pct = pkg_pass / total_packages * 100.0 if total_packages else 0.0
        file_pct = file_pass / total_files * 100.0 if total_files else 0.0
        print(
            f"  {threshold:>9.2f}  "
            f"{pkg_pass:>4} ({pkg_pct:4.1f}%)  "
            f"{file_pass:>5} ({file_pct:4.1f}%)"
        )


def _group_scores_by_label(
    stats: dict[str, dict],
    label_map: dict[str, str],
) -> dict[str, list[float]]:
    """Group all file-level scores by their package's usage label."""
    grouped: dict[str, list[float]] = defaultdict(list)
    for package, s in stats.items():
        label = label_map.get(package, "unlabeled")
        grouped[label].extend(s["scores"])
    return grouped


def print_label_stratification(
    stats: dict[str, dict],
    profiles: dict[str, str],
) -> None:
    """Print score distributions stratified by usage label."""
    grouped = _group_scores_by_label(stats, profiles)

    print("\nStratification by usage label:")
    for label in sorted(grouped.keys()):
        score_list = grouped[label]
        if not score_list:
            continue
        mean = statistics.mean(score_list)
        median = statistics.median(score_list)
        n_pkgs = sum(1 for p in stats if profiles.get(p, "unlabeled") == label)
        print(
            f"  {label:<20} n_packages={n_pkgs:<5} n_files={len(score_list):<6} "
            f"mean={mean:.1f}  median={median:.1f}"
        )


def print_evidence_analysis(
    stats: dict[str, dict],
    evidence: dict[str, dict],
) -> None:
    """Print mean structural score per signal_classification group and
    mean direct_dep_count per structural score quartile."""
    # Mean structural score per signal_classification.
    signal_scores: dict[str, list[float]] = defaultdict(list)
    for package, s in stats.items():
        ev = evidence.get(package)
        if ev and "signal_classification" in ev:
            sig = ev["signal_classification"]
            signal_scores[sig].extend(s["scores"])

    if signal_scores:
        print("\nMean structural score by evidence signal_classification:")
        for sig, scores in sorted(signal_scores.items()):
            mean = statistics.mean(scores) if scores else 0.0
            print(f"  {sig:<20} n_files={len(scores):<6} mean={mean:.1f}")

    # Mean direct_dep_count per structural score quartile.
    # Collect (score, dep_count) pairs.
    pairs: list[tuple[float, int]] = []
    for package, s in stats.items():
        ev = evidence.get(package)
        if ev and "direct_dep_count" in ev:
            dep_count = ev["direct_dep_count"]
            for score in s["scores"]:
                pairs.append((score, dep_count))

    if pairs:
        all_scores_sorted = sorted(p[0] for p in pairs)
        n = len(all_scores_sorted)
        q1 = all_scores_sorted[n // 4]
        q2 = all_scores_sorted[n // 2]
        q3 = all_scores_sorted[3 * n // 4]

        def _quartile_label(score: float) -> str:
            if score < q1:
                return f"Q1 (<{q1:.0f})"
            if score < q2:
                return f"Q2 ({q1:.0f}–{q2:.0f})"
            if score < q3:
                return f"Q3 ({q2:.0f}–{q3:.0f})"
            return f"Q4 (≥{q3:.0f})"

        quartile_deps: dict[str, list[int]] = defaultdict(list)
        for score, dep_count in pairs:
            quartile_deps[_quartile_label(score)].append(dep_count)

        print(
            "\nMean direct_dep_count (or evidence proxy) per structural score quartile:"
        )
        for quartile_label in sorted(quartile_deps.keys()):
            dep_list = quartile_deps[quartile_label]
            mean_deps = statistics.mean(dep_list) if dep_list else 0.0
            print(
                f"  {quartile_label:<20} n_files={len(dep_list):<6} "
                f"mean_dep_count={mean_deps:.1f}"
            )


# ---------------------------------------------------------------------------
# Summary JSON
# ---------------------------------------------------------------------------


def build_summary(
    stats: dict[str, dict],
    profiles: dict[str, str],
    evidence: dict[str, dict],
    *,
    filters: dict[str, str | None] | None = None,
) -> dict[str, Any]:
    """Build the JSON summary artifact."""
    all_scores: list[float] = []
    for s in stats.values():
        all_scores.extend(s["scores"])

    label_stats: dict[str, dict] = {}
    if profiles:
        grouped = _group_scores_by_label(stats, profiles)
        for label, scores in grouped.items():
            if scores:
                label_stats[label] = {
                    "n_files": len(scores),
                    "mean": statistics.mean(scores),
                    "median": statistics.median(scores),
                }

    threshold_sweep_results: list[dict] = []
    total_files = len(all_scores)
    total_packages = len(stats)
    for threshold in THRESHOLD_SWEEP:
        pkg_pass = sum(
            1
            for s in stats.values()
            if s["scores"] and statistics.mean(s["scores"]) / 100.0 >= threshold
        )
        file_pass = sum(1 for score in all_scores if score / 100.0 >= threshold)
        threshold_sweep_results.append(
            {
                "threshold": threshold,
                "packages_passing": pkg_pass,
                "packages_total": total_packages,
                "files_passing": file_pass,
                "files_total": total_files,
            }
        )

    per_package_summary = {
        package: {
            k: v
            for k, v in s.items()
            if k != "scores"  # omit raw score lists
        }
        for package, s in stats.items()
    }

    out: dict[str, Any] = {
        "total_packages": total_packages,
        "total_files": total_files,
        "overall_mean_structural": statistics.mean(all_scores) if all_scores else None,
        "overall_median_structural": statistics.median(all_scores)
        if all_scores
        else None,
        "threshold_sweep": threshold_sweep_results,
        "by_label": label_stats,
        "per_package": per_package_summary,
    }
    if filters:
        out["filters"] = {k: v for k, v in filters.items() if v is not None}
    return out


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------


def _build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description=(
            "Analyze structural_scores.jsonl from run_structural_baseline*.py. "
            "Prints per-package stats, threshold sweep, and optional stratification."
        )
    )
    parser.add_argument(
        "--scores",
        type=Path,
        default=RESULTS_DIR / "structural_scores.jsonl",
        help="Path to structural_scores.jsonl. "
        "Default: evaluations/calibration/results/structural_scores.jsonl",
    )
    parser.add_argument(
        "--profiles",
        type=Path,
        default=CALIBRATION_DIR / "usage_profiles.csv",
        help="Path to usage_profiles.csv. "
        "Default: evaluations/calibration/usage_profiles.csv",
    )
    parser.add_argument(
        "--evidence",
        type=Path,
        default=CALIBRATION_DIR / "evidence" / "pypi_evidence.jsonl",
        help="Path to evidence JSONL (PyPI, crates.io, npm, or vcpkg format). "
        "Default: evaluations/calibration/evidence/pypi_evidence.jsonl",
    )
    parser.add_argument(
        "--ecosystem",
        default=None,
        help="If set, keep only JSONL score rows whose ``ecosystem`` field matches.",
    )
    parser.add_argument(
        "--language",
        default=None,
        help="If set, keep only JSONL score rows whose ``language`` field matches.",
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=RESULTS_DIR / "score_analysis.json",
        help="Path for JSON summary output. "
        "Default: evaluations/calibration/results/score_analysis.json",
    )
    return parser


def main() -> None:
    """Entry point for analyze_scores."""
    parser = _build_parser()
    args = parser.parse_args()

    print(f"Loading scores from {args.scores} ...")
    records = load_scores(
        args.scores,
        ecosystem=args.ecosystem,
        language=args.language,
    )
    print(f"  Loaded {len(records)} file records.")

    stats = compute_package_stats(records)
    print(f"  Grouped into {len(stats)} packages.")

    profiles = load_profiles(args.profiles)
    if profiles:
        print(f"  Loaded {len(profiles)} usage labels from {args.profiles}")
    else:
        print(f"  No usage_profiles.csv at {args.profiles} — skipping stratification")

    evidence = load_evidence(args.evidence)
    if evidence:
        print(f"  Loaded evidence for {len(evidence)} packages from {args.evidence}")
    else:
        print(
            f"  No evidence JSONL found at {args.evidence} — skipping evidence analysis"
        )

    print()
    print("=" * 72)
    print("Per-package structural scores")
    print("=" * 72)
    print_package_table(stats)

    print_threshold_sweep(stats)

    if profiles:
        print_label_stratification(stats, profiles)

    if evidence:
        print_evidence_analysis(stats, evidence)

    # Write JSON summary.
    summary = build_summary(
        stats,
        profiles,
        evidence,
        filters={"ecosystem": args.ecosystem, "language": args.language},
    )
    output_path: Path = args.output
    output_path.parent.mkdir(parents=True, exist_ok=True)
    output_path.write_text(json.dumps(summary, indent=2), encoding="utf-8")
    print(f"\nWrote summary to {output_path}")


if __name__ == "__main__":
    main()
