"""Evaluate two source versions for popular Python libraries with Topos."""

from __future__ import annotations

import argparse
import json
import tarfile
import zipfile
from collections import Counter
from dataclasses import asdict, dataclass
from datetime import UTC, datetime
from pathlib import Path
from typing import Any
from urllib.error import HTTPError, URLError
from urllib.request import urlopen

from topos.core.omega import EvaluationValue
from topos.evaluation.policies.base import Priority
from topos.evaluation.preferences import UserPreferences, default_preferences
from topos.mcp.evaluation import classify_file

ARCHIVE_SUFFIXES = (".tar.gz", ".tar.bz2", ".tar.xz", ".tgz", ".zip", ".whl", ".tar")
EVALUATION_ORDER = tuple(value.name for value in EvaluationValue)


@dataclass(frozen=True)
class LibraryTarget:
    project_name: str
    import_dir: str
    versions: tuple[str, str]


@dataclass
class VersionSummary:
    library: str
    version: str
    package_path: str
    files_evaluated: int
    overall: str
    counts: dict[str, int]
    avg_scores: dict[str, float]
    avg_raw_metrics: dict[str, float]
    preference_target: str
    preference_fallback_target: str
    avg_preference_progress: float
    next_step_counts: dict[str, int]


VERSION_TARGETS: dict[str, LibraryTarget] = {
    "numpy": LibraryTarget(
        project_name="numpy",
        import_dir="numpy",
        versions=("1.26.4", "2.4.4"),
    ),
    "scipy": LibraryTarget(
        project_name="scipy",
        import_dir="scipy",
        versions=("1.11.4", "1.17.1"),
    ),
    "scikit-learn": LibraryTarget(
        project_name="scikit-learn",
        import_dir="sklearn",
        versions=("1.4.2", "1.8.0"),
    ),
    "networkx": LibraryTarget(
        project_name="networkx",
        import_dir="networkx",
        versions=("2.8.8", "3.6.1"),
    ),
}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Download two versions of each target library, then run topos evaluate "
            "on the library source directory."
        )
    )
    parser.add_argument(
        "--library",
        action="append",
        choices=tuple(VERSION_TARGETS.keys()),
        help="Limit execution to one or more libraries.",
    )
    parser.add_argument(
        "--cache-dir",
        type=Path,
        default=Path("demos/libraries/.cache"),
        help="Where downloaded archives and extracted sources are stored.",
    )
    parser.add_argument(
        "--write-json",
        action="store_true",
        help="Write summary JSON artifact.",
    )
    parser.add_argument(
        "--json-output",
        type=Path,
        default=Path("demos/libraries/results/version_summaries.json"),
        help="Output path for summary JSON when --write-json is enabled.",
    )
    parser.add_argument(
        "--skip-download",
        action="store_true",
        help="Do not run pip download (requires archives in cache).",
    )
    return parser.parse_args()


def repo_root() -> Path:
    return Path(__file__).resolve().parents[2]


def canonicalize(name: str) -> str:
    return name.lower().replace("-", "_").replace(".", "_")


def is_archive(path: Path) -> bool:
    return any(path.name.endswith(suffix) for suffix in ARCHIVE_SUFFIXES)


def matches_archive(path: Path, project_name: str, version: str) -> bool:
    lowered = path.name.lower()
    project_key = canonicalize(project_name)
    version_key = version.lower().replace("-", "_")
    return is_archive(path) and project_key in lowered and version_key in lowered


def ensure_archive(
    project_name: str,
    version: str,
    download_dir: Path,
    *,
    skip_download: bool,
) -> Path:
    download_dir.mkdir(parents=True, exist_ok=True)
    existing = sorted(
        [
            path
            for path in download_dir.iterdir()
            if path.is_file() and matches_archive(path, project_name, version)
        ],
        key=lambda path: path.stat().st_mtime,
        reverse=True,
    )
    if existing:
        return existing[0]

    if skip_download:
        raise FileNotFoundError(
            f"No cached archive for {project_name}=={version} in {download_dir}"
        )

    distribution = fetch_distribution(project_name, version)
    target = download_dir / distribution["filename"]
    if target.exists():
        return target

    download_distribution(distribution["url"], target)
    return target


def extract_archive(archive: Path, destination: Path) -> None:
    destination.mkdir(parents=True, exist_ok=True)
    if any(destination.iterdir()):
        return

    if archive.name.endswith((".zip", ".whl")):
        with zipfile.ZipFile(archive) as zip_handle:
            zip_handle.extractall(destination)
        return

    with tarfile.open(archive) as tar_handle:
        tar_handle.extractall(destination, filter="data")


def locate_import_dir(extracted_root: Path, import_dir: str) -> Path:
    direct = extracted_root / import_dir
    if direct.is_dir() and any(direct.rglob("*.py")):
        return direct

    candidates: list[Path] = []
    for candidate in extracted_root.rglob(import_dir):
        if not candidate.is_dir():
            continue
        if (candidate / "__init__.py").exists() and any(candidate.rglob("*.py")):
            candidates.append(candidate)

    if not candidates:
        raise FileNotFoundError(
            f"Could not locate import directory '{import_dir}' under {extracted_root}"
        )

    candidates.sort(
        key=lambda path: (len(path.relative_to(extracted_root).parts), str(path))
    )
    return candidates[0]


def fetch_distribution(project_name: str, version: str) -> dict[str, str]:
    endpoint = f"https://pypi.org/pypi/{project_name}/{version}/json"
    try:
        with urlopen(endpoint) as response:
            payload = json.loads(response.read().decode("utf-8"))
    except HTTPError as error:
        raise RuntimeError(
            f"PyPI lookup failed for {project_name}=={version}: {error}"
        ) from error
    except URLError as error:
        raise RuntimeError(
            f"Network failure while fetching {project_name}=={version}: {error}"
        ) from error

    files: list[dict[str, str]] = payload.get("urls", [])
    if not files:
        raise RuntimeError(f"No files listed for {project_name}=={version}")

    wheels = [item for item in files if item.get("packagetype") == "bdist_wheel"]
    if wheels:
        py3_wheels = [
            item
            for item in wheels
            if str(item.get("filename", "")).endswith("py3-none-any.whl")
        ]
        choice = py3_wheels[0] if py3_wheels else wheels[0]
        return {"filename": choice["filename"], "url": choice["url"]}

    sdists = [item for item in files if item.get("packagetype") == "sdist"]
    if sdists:
        return {"filename": sdists[0]["filename"], "url": sdists[0]["url"]}

    raise RuntimeError(f"No downloadable wheel/sdist for {project_name}=={version}")


def download_distribution(url: str, destination: Path) -> None:
    destination.parent.mkdir(parents=True, exist_ok=True)
    try:
        with urlopen(url) as response:
            destination.write_bytes(response.read())
    except URLError as error:
        raise RuntimeError(
            f"Failed to download distribution from {url}: {error}"
        ) from error


def _result_to_record(
    path: Path,
    package_path: Path,
    *,
    priority: Priority,
    preferences: UserPreferences,
) -> dict[str, Any]:
    result, dep_graph = classify_file(path, priority, gitnexus_dir=None)
    current = result.summary()
    next_step = preferences.next_step(current)
    return {
        "file": str(path.relative_to(package_path)),
        "lattice_element": current.name,
        "lattice_symbol": current.symbol,
        "dimensions": {dim: value.name for dim, value in result.dimensions.items()},
        "scores": {dim: score * 100.0 for dim, score in result.scores.items()},
        "raw_metrics": dict(result.raw_metrics),
        "is_parseable": result.is_parseable,
        "coupling_available": dep_graph is not None,
        "preference_walk": {
            "ranking": [generator.value for generator in preferences.ranking],
            "target": preferences.resolved_target().name,
            "fallback_target": preferences.fallback_target().name,
            "walk": [value.name for value in preferences.relaxation_walk(current)],
            "next_step": next_step.name if next_step is not None else None,
            "progress": preferences.progress(current),
        },
    }


def evaluate_python_package(
    package_path: Path,
    *,
    priority: Priority = Priority.SECURE,
    preferences: UserPreferences | None = None,
) -> tuple[list[dict[str, Any]], str]:
    """Evaluate every Python file under ``package_path`` with current Topos APIs."""
    prefs = preferences or default_preferences()
    results: list[dict[str, Any]] = []
    for path in sorted(package_path.rglob("*.py")):
        try:
            results.append(
                _result_to_record(
                    path,
                    package_path,
                    priority=priority,
                    preferences=prefs,
                )
            )
        except Exception as exc:  # noqa: BLE001 - record per-file demo failures
            results.append(
                {
                    "file": str(path.relative_to(package_path)),
                    "lattice_element": EvaluationValue.SLOP.name,
                    "lattice_symbol": EvaluationValue.SLOP.symbol,
                    "dimensions": {},
                    "scores": {},
                    "raw_metrics": {},
                    "is_parseable": False,
                    "coupling_available": False,
                    "error": str(exc),
                    "preference_walk": {
                        "ranking": [g.value for g in prefs.ranking],
                        "target": prefs.resolved_target().name,
                        "fallback_target": prefs.fallback_target().name,
                        "walk": [
                            value.name
                            for value in prefs.relaxation_walk(EvaluationValue.SLOP)
                        ],
                        "next_step": (
                            prefs.next_step(EvaluationValue.SLOP).name
                            if prefs.next_step(EvaluationValue.SLOP) is not None
                            else None
                        ),
                        "progress": prefs.progress(EvaluationValue.SLOP),
                    },
                }
            )

    achieved = {
        "simple": bool(results)
        and all(
            r.get("dimensions", {}).get("simple") in (EvaluationValue.SIMPLE.name, EvaluationValue.SIMPLE_COMPOSABLE.name, EvaluationValue.SIMPLE_SECURE.name, EvaluationValue.IDEAL.name)
            for r in results
        ),
        "composable": bool(results)
        and all(
            r.get("dimensions", {}).get("composable") in (EvaluationValue.COMPOSABLE.name, EvaluationValue.SIMPLE_COMPOSABLE.name, EvaluationValue.COMPOSABLE_SECURE.name, EvaluationValue.IDEAL.name)
            for r in results
        ),
        "secure": bool(results)
        and all(
            r.get("dimensions", {}).get("secure") in (EvaluationValue.SECURE.name, EvaluationValue.SIMPLE_SECURE.name, EvaluationValue.COMPOSABLE_SECURE.name, EvaluationValue.IDEAL.name)
            for r in results
        ),
    }
    overall = EvaluationValue.SLOP
    from topos.core.omega import verdict_from_generators

    if results:
        overall = verdict_from_generators(**achieved)
    return results, overall.name


def summarize_results(
    library: str,
    version: str,
    package_path: Path,
    results: list[dict[str, Any]],
    overall: str,
) -> VersionSummary:
    counts = Counter(
        result.get("lattice_element") or EvaluationValue.SLOP.name
        for result in results
        if isinstance(result, dict)
    )

    score_values: dict[str, list[float]] = {}
    raw_values: dict[str, list[float]] = {}
    progress_values: list[float] = []
    next_steps: Counter[str] = Counter()
    preference_target = default_preferences().resolved_target().name
    preference_fallback_target = default_preferences().fallback_target().name

    for result in results:
        scores = result.get("scores", {})
        if isinstance(scores, dict):
            for key, value in scores.items():
                if isinstance(value, (int, float)):
                    score_values.setdefault(key, []).append(float(value))
        raw_metrics = result.get("raw_metrics", {})
        if isinstance(raw_metrics, dict):
            for key, value in raw_metrics.items():
                if isinstance(value, (int, float)):
                    raw_values.setdefault(key, []).append(float(value))
        walk = result.get("preference_walk", {})
        if isinstance(walk, dict):
            if isinstance(walk.get("progress"), (int, float)):
                progress_values.append(float(walk["progress"]))
            if isinstance(walk.get("target"), str):
                preference_target = walk["target"]
            if isinstance(walk.get("fallback_target"), str):
                preference_fallback_target = walk["fallback_target"]
            next_step = walk.get("next_step")
            if isinstance(next_step, str):
                next_steps[next_step] += 1

    avg_scores = {
        key: sum(values) / len(values) for key, values in sorted(score_values.items())
    }
    avg_raw_metrics = {
        key: sum(values) / len(values) for key, values in sorted(raw_values.items())
    }

    normalized_counts = {label: counts.get(label, 0) for label in EVALUATION_ORDER}

    return VersionSummary(
        library=library,
        version=version,
        package_path=str(package_path),
        files_evaluated=len(results),
        overall=overall,
        counts=normalized_counts,
        avg_scores=avg_scores,
        avg_raw_metrics=avg_raw_metrics,
        preference_target=preference_target,
        preference_fallback_target=preference_fallback_target,
        avg_preference_progress=(
            sum(progress_values) / len(progress_values) if progress_values else 0.0
        ),
        next_step_counts=dict(next_steps),
    )


def print_summary(summary: dict[str, list[VersionSummary]]) -> None:
    print("Topos version-evaluation summary")
    print("=" * 72)
    for library, version_summaries in summary.items():
        print()
        print(library)
        print("-" * 72)
        for item in version_summaries:
            non_zero_counts = ", ".join(
                f"{label}:{item.counts[label]}"
                for label in EVALUATION_ORDER
                if item.counts[label] > 0
            )
            if not non_zero_counts:
                non_zero_counts = "(none)"
            score_str = ", ".join(
                f"{dim}={score:.1f}%" for dim, score in item.avg_scores.items()
            )
            if not score_str:
                score_str = "(no scores)"
            next_steps = ", ".join(
                f"{step}:{count}" for step, count in item.next_step_counts.items()
            )
            if not next_steps:
                next_steps = "at-target"
            print(
                f"{item.version:8} overall={item.overall:14} "
                f"files={item.files_evaluated:<5} "
                f"preference_progress={item.avg_preference_progress:.2f} "
                f"target={item.preference_target} "
                f"fallback={item.preference_fallback_target}"
            )
            print(f"          counts=[{non_zero_counts}]")
            print(f"          avg_scores=[{score_str}]")
            print(f"          next_steps=[{next_steps}]")

        if len(version_summaries) == 2:
            old, new = version_summaries
            score_keys = sorted(set(old.avg_scores) | set(new.avg_scores))
            deltas = []
            for key in score_keys:
                delta = new.avg_scores.get(key, 0.0) - old.avg_scores.get(key, 0.0)
                deltas.append(f"{key}={delta:+.1f}%")
            print("delta     " + ", ".join(deltas))


def write_json(
    output: Path,
    *,
    selected_libraries: list[str],
    summary: dict[str, list[VersionSummary]],
) -> None:
    output.parent.mkdir(parents=True, exist_ok=True)
    payload = {
        "generated_at": datetime.now(UTC).isoformat(),
        "libraries": selected_libraries,
        "results": {
            library: [asdict(item) for item in items]
            for library, items in summary.items()
        },
    }
    output.write_text(json.dumps(payload, indent=2), encoding="utf-8")
    print()
    print(f"Wrote JSON artifact: {output}")


def evaluate_library_versions(
    library: str,
    target: LibraryTarget,
    *,
    cache_dir: Path,
    skip_download: bool,
) -> list[VersionSummary]:
    download_dir = cache_dir / "downloads"
    extract_base = cache_dir / "sources" / canonicalize(target.project_name)
    summaries: list[VersionSummary] = []

    for version in target.versions:
        print(f"Preparing {target.project_name}=={version}...")
        archive = ensure_archive(
            target.project_name,
            version,
            download_dir,
            skip_download=skip_download,
        )

        extract_dir = extract_base / version
        extract_archive(archive, extract_dir)

        package_dir = locate_import_dir(extract_dir, target.import_dir)
        print(f"Evaluating {library} {version} from {package_dir} ...")
        results, overall = evaluate_python_package(
            package_dir,
            priority=Priority.SECURE,
            preferences=default_preferences(),
        )
        summaries.append(
            summarize_results(
                library=library,
                version=version,
                package_path=package_dir,
                results=results,
                overall=overall,
            )
        )

    return summaries


def main() -> None:
    args = parse_args()
    selected_libraries = args.library or list(VERSION_TARGETS.keys())

    summary: dict[str, list[VersionSummary]] = {}
    for library in selected_libraries:
        summary[library] = evaluate_library_versions(
            library,
            VERSION_TARGETS[library],
            cache_dir=args.cache_dir,
            skip_download=args.skip_download,
        )

    print()
    print_summary(summary)

    if args.write_json:
        write_json(
            args.json_output,
            selected_libraries=selected_libraries,
            summary=summary,
        )


if __name__ == "__main__":
    main()
