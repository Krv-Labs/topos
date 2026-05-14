"""Download each package from PyPI and run ``topos evaluate --json`` on it.

For each package in the cohort, fetches the latest version from PyPI,
downloads and extracts the sdist/wheel, locates the primary Python source
directory, runs ``topos evaluate -r --json --priority secure``, and writes
per-file results to a JSONL file.

Reuses the download helpers from ``demos/libraries/run_all.py``.

Run:
    python evaluations/calibration/scripts/run_structural_baseline.py
    python evaluations/calibration/scripts/run_structural_baseline.py \\
        --package requests --package httpx
    python evaluations/calibration/scripts/run_structural_baseline.py --limit 10
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import urllib.error
import urllib.request
from pathlib import Path

# ---------------------------------------------------------------------------
# Path constants
# ---------------------------------------------------------------------------

REPO_ROOT = Path(__file__).resolve().parents[3]
CALIBRATION_DIR = REPO_ROOT / "evaluations" / "calibration"
RESULTS_DIR = CALIBRATION_DIR / "results"
CACHE_DIR = CALIBRATION_DIR / ".cache"

# Directories to skip when searching for a primary source package.
SKIP_DIRS: frozenset[str] = frozenset(
    {"tests", "test", "docs", "build", "dist", "examples", "benchmarks", "scripts"}
)

# ---------------------------------------------------------------------------
# Inject demos/libraries so we can reuse PyPI download helpers.
# ---------------------------------------------------------------------------

_DEMOS_LIBRARIES = REPO_ROOT / "demos" / "libraries"
if str(_DEMOS_LIBRARIES) not in sys.path:
    sys.path.insert(0, str(_DEMOS_LIBRARIES))

from run_all import (  # type: ignore[import-not-found]  # noqa: E402
    canonicalize,
    ensure_archive,
    extract_archive,
)


# ---------------------------------------------------------------------------
# PyPI version lookup
# ---------------------------------------------------------------------------


def fetch_latest_version(package: str) -> str:
    """Query PyPI and return the latest stable version string for *package*."""
    url = f"https://pypi.org/pypi/{package}/json"
    try:
        with urllib.request.urlopen(url, timeout=15) as response:
            data = json.loads(response.read().decode("utf-8"))
        version: str = data["info"]["version"]
        return version
    except urllib.error.HTTPError as exc:
        raise RuntimeError(
            f"PyPI lookup failed for {package}: HTTP {exc.code} {exc.reason}"
        ) from exc
    except urllib.error.URLError as exc:
        raise RuntimeError(
            f"Network failure fetching {package}: {exc.reason}"
        ) from exc


# ---------------------------------------------------------------------------
# Source directory discovery
# ---------------------------------------------------------------------------


def find_source_dir(extracted_root: Path, package: str) -> Path:
    """Locate the primary Python source directory inside an extracted archive.

    Search order:
    1. ``{extracted_root}/{canonical}/``
    2. ``{extracted_root}/src/{canonical}/``
    3. First subdirectory containing ``__init__.py`` that is not in SKIP_DIRS.

    Raises
    ------
    FileNotFoundError
        When no suitable directory is found.
    """
    canonical = canonicalize(package)

    # Strategy 1: direct match.
    candidate = extracted_root / canonical
    if candidate.is_dir() and any(candidate.rglob("*.py")):
        return candidate

    # Strategy 2: src layout.
    candidate = extracted_root / "src" / canonical
    if candidate.is_dir() and any(candidate.rglob("*.py")):
        return candidate

    # Strategy 3: first subdir with __init__.py that is not a noise dir.
    def _is_candidate(path: Path) -> bool:
        if not path.is_dir():
            return False
        if path.name.lower() in SKIP_DIRS:
            return False
        return (path / "__init__.py").exists()

    # Check direct children first, then recurse one level into "src".
    search_roots = [extracted_root, extracted_root / "src"]
    for search_root in search_roots:
        if not search_root.is_dir():
            continue
        candidates = sorted(
            [child for child in search_root.iterdir() if _is_candidate(child)],
            key=lambda p: p.name,
        )
        if candidates:
            return candidates[0]

    raise FileNotFoundError(
        f"Could not find a primary source directory for '{package}' "
        f"under {extracted_root}"
    )


# ---------------------------------------------------------------------------
# topos evaluate runner
# ---------------------------------------------------------------------------


def run_topos_evaluate(path: Path) -> dict:
    """Run ``topos evaluate <path> -r --json --priority secure``.

    Sets PYTHONPATH to include ``{REPO_ROOT}/src`` so the installed editable
    package is found.  Strips the trailing ``"Overall:"`` line that the CLI
    appends in recursive mode before JSON-parsing.

    Returns
    -------
    dict
        Parsed JSON payload from topos evaluate.

    Raises
    ------
    RuntimeError
        On non-zero exit code or unparseable output.
    """
    env = dict(os.environ)
    src_path = str(REPO_ROOT / "src")
    existing = env.get("PYTHONPATH", "")
    env["PYTHONPATH"] = src_path if not existing else f"{src_path}{os.pathsep}{existing}"

    command = [
        sys.executable,
        "-m",
        "topos.cli.main",
        "evaluate",
        str(path),
        "-r",
        "--json",
        "--priority",
        "secure",
    ]

    completed = subprocess.run(
        command,
        cwd=REPO_ROOT,
        env=env,
        text=True,
        capture_output=True,
        check=False,
    )

    if completed.returncode != 0:
        raise RuntimeError(
            f"topos evaluate failed for {path}\n"
            f"stdout:\n{completed.stdout}\nstderr:\n{completed.stderr}"
        )

    payload_text = completed.stdout
    marker = "\n\nOverall:"
    if marker in payload_text:
        payload_text = payload_text.split(marker, maxsplit=1)[0]

    try:
        return json.loads(payload_text.strip())
    except json.JSONDecodeError as exc:
        raise RuntimeError(
            f"Failed to parse topos output for {path}: {exc}\n"
            f"Raw output:\n{completed.stdout[:500]}"
        ) from exc


# ---------------------------------------------------------------------------
# Per-package processing
# ---------------------------------------------------------------------------


def process_package(
    package: str,
    *,
    skip_download: bool,
    output_fh,
) -> None:
    """Download, extract, evaluate, and write results for one package."""
    version = fetch_latest_version(package)
    print(f"  version={version}", end="", flush=True)

    download_dir = CACHE_DIR / "downloads"
    extract_base = CACHE_DIR / "sources" / canonicalize(package)
    extract_dir = extract_base / version

    archive = ensure_archive(
        package,
        version,
        download_dir,
        skip_download=skip_download,
    )

    extract_archive(archive, extract_dir)

    # Most sdists unpack into a single top-level directory.
    children = [p for p in extract_dir.iterdir() if p.is_dir()]
    extracted_root = children[0] if len(children) == 1 else extract_dir

    src_dir = find_source_dir(extracted_root, package)
    print(f" src={src_dir.relative_to(extract_dir)}", end="", flush=True)

    payload = run_topos_evaluate(src_dir)
    results: list[dict] = payload.get("results", [])

    print(f" files={len(results)}")

    for file_result in results:
        if isinstance(file_result, dict):
            record = {"package": package, "version": version, **file_result}
        else:
            record = {"package": package, "version": version, "raw": file_result}
        output_fh.write(json.dumps(record) + "\n")


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------


def _build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description=(
            "Download each package from PyPI and run topos evaluate --json on "
            "the primary source directory. Writes per-file results to JSONL."
        )
    )
    parser.add_argument(
        "--package",
        action="append",
        dest="packages",
        metavar="PACKAGE",
        help="One or more package names to process (may be repeated). "
        "Defaults to the full cohort.",
    )
    parser.add_argument(
        "--cohort",
        type=Path,
        default=CALIBRATION_DIR / "top100_pypi.txt",
        help="Path to the package list (one name per line). "
        "Default: evaluations/calibration/top100_pypi.txt",
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=RESULTS_DIR / "structural_scores.jsonl",
        help="Output JSONL path. "
        "Default: evaluations/calibration/results/structural_scores.jsonl",
    )
    parser.add_argument(
        "--limit",
        type=int,
        default=None,
        metavar="N",
        help="Process only the first N packages. Useful for quick tests.",
    )
    parser.add_argument(
        "--skip-download",
        action="store_true",
        help="Skip download if archive is already cached.",
    )
    return parser


def main() -> None:
    """Entry point for run_structural_baseline."""
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

    if args.limit is not None:
        packages = packages[: args.limit]

    output_path: Path = args.output
    output_path.parent.mkdir(parents=True, exist_ok=True)
    CACHE_DIR.mkdir(parents=True, exist_ok=True)

    total = len(packages)
    print(f"Processing {total} packages → {output_path}")
    print()

    with output_path.open("w", encoding="utf-8") as out_fh:
        for idx, package in enumerate(packages, start=1):
            print(f"[{idx}/{total}] {package} ...", end=" ", flush=True)
            try:
                process_package(
                    package,
                    skip_download=args.skip_download,
                    output_fh=out_fh,
                )
            except Exception as exc:  # noqa: BLE001
                error_msg = str(exc).splitlines()[0][:200]
                print(f" ERROR: {error_msg}")
                error_record = {
                    "package": package,
                    "version": "unknown",
                    "error": str(exc),
                }
                out_fh.write(json.dumps(error_record) + "\n")

    print()
    print(f"Done. Results written to {output_path}")


if __name__ == "__main__":
    main()
