"""Evaluate two source versions for popular Python libraries with Topos."""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import tarfile
import zipfile
from collections import Counter
from dataclasses import asdict, dataclass
from datetime import UTC, datetime
from pathlib import Path
from typing import Any
from urllib.error import HTTPError, URLError
from urllib.request import urlopen

ARCHIVE_SUFFIXES = (".tar.gz", ".tar.bz2", ".tar.xz", ".tgz", ".zip", ".whl", ".tar")
EVALUATION_ORDER = ("BROKEN", "COMPOSABLE", "SELF_CONTAINED", "SOUND")


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
    avg_complexity: float
    avg_entropy: float


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
        default=Path("demos/.cache"),
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
        default=Path("demos/results/version_summaries.json"),
        help="Output path for summary JSON when --write-json is enabled.",
    )
    parser.add_argument(
        "--skip-download",
        action="store_true",
        help="Do not run pip download (requires archives in cache).",
    )
    return parser.parse_args()


def repo_root() -> Path:
    return Path(__file__).resolve().parents[1]


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


def topos_env(root: Path) -> dict[str, str]:
    env = dict(os.environ)
    src = str(root / "src")
    existing = env.get("PYTHONPATH", "")
    env["PYTHONPATH"] = src if not existing else f"{src}{os.pathsep}{existing}"
    return env


def run_topos_evaluate(path: Path, *, root: Path, env: dict[str, str]) -> str:
    command = [
        sys.executable,
        "-m",
        "topos.main",
        "evaluate",
        str(path),
        "-r",
        "--json",
    ]
    completed = subprocess.run(
        command,
        cwd=root,
        env=env,
        text=True,
        capture_output=True,
        check=False,
    )
    if completed.returncode != 0:
        raise RuntimeError(
            f"Topos evaluate failed for {path}\n"
            f"stdout:\n{completed.stdout}\n"
            f"stderr:\n{completed.stderr}"
        )
    return completed.stdout


def parse_evaluate_output(output: str) -> tuple[list[dict[str, Any]], str]:
    marker = "\n\nOverall:"
    overall = "UNKNOWN"
    json_text = output

    if marker in output:
        json_text, trailing = output.split(marker, maxsplit=1)
        overall = trailing.strip()

    payload = json.loads(json_text.strip())
    results = payload.get("results", [])
    if not isinstance(results, list):
        raise ValueError("Unexpected topos JSON payload: results is not a list")
    return results, overall


def summarize_results(
    library: str,
    version: str,
    package_path: Path,
    results: list[dict[str, Any]],
    overall: str,
) -> VersionSummary:
    counts = Counter(
        result.get("summary", "BROKEN")
        for result in results
        if isinstance(result, dict)
    )
    complexities = [
        float(result["complexity"])
        for result in results
        if isinstance(result, dict)
        and isinstance(result.get("complexity"), (int, float))
    ]
    entropies = [
        float(result["entropy"])
        for result in results
        if isinstance(result, dict) and isinstance(result.get("entropy"), (int, float))
    ]

    avg_complexity = sum(complexities) / len(complexities) if complexities else 0.0
    avg_entropy = sum(entropies) / len(entropies) if entropies else 0.0

    normalized_counts = {label: counts.get(label, 0) for label in EVALUATION_ORDER}

    return VersionSummary(
        library=library,
        version=version,
        package_path=str(package_path),
        files_evaluated=len(results),
        overall=overall,
        counts=normalized_counts,
        avg_complexity=avg_complexity,
        avg_entropy=avg_entropy,
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
            print(
                f"{item.version:8} overall={item.overall:14} "
                f"files={item.files_evaluated:<5} "
                f"avg_complexity={item.avg_complexity:.3f} "
                f"avg_entropy={item.avg_entropy:.3f}"
            )
            print(f"          counts=[{non_zero_counts}]")

        if len(version_summaries) == 2:
            old, new = version_summaries
            complexity_delta = new.avg_complexity - old.avg_complexity
            entropy_delta = new.avg_entropy - old.avg_entropy
            print(
                "delta     "
                f"avg_complexity={complexity_delta:+.3f} "
                f"avg_entropy={entropy_delta:+.3f}"
            )


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
    root: Path,
    cache_dir: Path,
    env: dict[str, str],
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
        output = run_topos_evaluate(package_dir, root=root, env=env)
        results, overall = parse_evaluate_output(output)
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
    root = repo_root()
    env = topos_env(root)
    selected_libraries = args.library or list(VERSION_TARGETS.keys())

    summary: dict[str, list[VersionSummary]] = {}
    for library in selected_libraries:
        summary[library] = evaluate_library_versions(
            library,
            VERSION_TARGETS[library],
            root=root,
            cache_dir=args.cache_dir,
            env=env,
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
