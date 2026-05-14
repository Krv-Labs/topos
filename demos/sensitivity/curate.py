"""Curate SIMPLE and COMPOSABLE reference corpora from popular Python packages.

Downloads sdists/wheels from PyPI, extracts target files / package slices into
``demos/sensitivity/corpus/``, scores each baseline with the current Topos
classifier, and writes ``manifest.json`` describing what was pinned.

Reuses the PyPI download helpers from ``demos/libraries/run_all.py``.
"""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path

# Reuse PyPI download helpers from the sibling demo.
_DEMOS_DIR = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(_DEMOS_DIR / "libraries"))
from run_all import (  # type: ignore[import-not-found]  # noqa: E402
    canonicalize,
    ensure_archive,
    extract_archive,
)

from topos.evaluation.policies.base import Priority  # noqa: E402
from topos.mcp.evaluation import classify_file  # noqa: E402


@dataclass(frozen=True)
class SimpleCandidate:
    """A single source file pulled from a PyPI package."""

    name: str
    package: str
    version: str
    relative_source: str  # path inside the extracted sdist, e.g. "tabulate/__init__.py"
    smoke_test: str  # python statement that exits 0 iff parseable


@dataclass(frozen=True)
class ComposableCandidate:
    """A multi-module package slice from PyPI."""

    name: str
    package: str
    version: str
    relative_package_dir: str  # directory inside the extracted sdist, e.g. "toolz"
    smoke_test: str


_PARSE_FILE_SMOKE = "import ast, pathlib; ast.parse(pathlib.Path(SRC).read_text())"
_PARSE_PKG_SMOKE = (
    "import ast, pathlib;"
    " [ast.parse(p.read_text()) for p in pathlib.Path(PKG).rglob('*.py')]"
)

# Three baselines spanning a complexity gradient so we can observe lattice
# transitions in both directions when noise is applied.
SIMPLE_CANDIDATES: tuple[SimpleCandidate, ...] = (
    SimpleCandidate(
        name="toolz_recipes",
        package="toolz",
        version="0.12.1",
        relative_source="toolz/recipes.py",
        smoke_test=_PARSE_FILE_SMOKE,
    ),
    SimpleCandidate(
        name="humanize_number",
        package="humanize",
        version="4.10.0",
        relative_source="src/humanize/number.py",
        smoke_test=_PARSE_FILE_SMOKE,
    ),
    SimpleCandidate(
        name="tabulate_init",
        package="tabulate",
        version="0.9.0",
        relative_source="tabulate/__init__.py",
        smoke_test=_PARSE_FILE_SMOKE,
    ),
)

COMPOSABLE_CANDIDATES: tuple[ComposableCandidate, ...] = (
    ComposableCandidate(
        name="toolz",
        package="toolz",
        version="0.12.1",
        relative_package_dir="toolz",
        smoke_test=_PARSE_PKG_SMOKE,
    ),
    ComposableCandidate(
        name="funcy",
        package="funcy",
        version="2.0",
        relative_package_dir="funcy",
        smoke_test=_PARSE_PKG_SMOKE,
    ),
)


def repo_root() -> Path:
    return Path(__file__).resolve().parents[2]


def sensitivity_dir() -> Path:
    return repo_root() / "demos" / "sensitivity"


def cache_dir() -> Path:
    return sensitivity_dir() / ".cache"


def fetch_extracted(package: str, version: str) -> Path:
    """Download (if needed) and extract the sdist; return the extracted root."""
    download_dir = cache_dir() / "downloads"
    extract_base = cache_dir() / "sources" / canonicalize(package) / version
    archive = ensure_archive(package, version, download_dir, skip_download=False)
    extract_archive(archive, extract_base)
    # Most sdists extract into a single top-level dir like "<package>-<version>".
    children = [p for p in extract_base.iterdir() if p.is_dir()]
    if len(children) == 1:
        return children[0]
    return extract_base


def locate_source(extracted_root: Path, relative_source: str) -> Path:
    candidate = extracted_root / relative_source
    if candidate.is_file():
        return candidate
    matches = list(extracted_root.rglob(Path(relative_source).name))
    if not matches:
        raise FileNotFoundError(
            f"Could not find {relative_source} under {extracted_root}"
        )
    matches.sort(key=lambda p: len(p.relative_to(extracted_root).parts))
    return matches[0]


def locate_package_dir(extracted_root: Path, relative_pkg: str) -> Path:
    candidate = extracted_root / relative_pkg
    if candidate.is_dir() and (candidate / "__init__.py").exists():
        return candidate
    for path in extracted_root.rglob(relative_pkg):
        if path.is_dir() and (path / "__init__.py").exists():
            return path
    raise FileNotFoundError(
        f"Could not find package dir {relative_pkg} under {extracted_root}"
    )


def smoke_check(
    smoke_test: str,
    *,
    src: Path | None = None,
    pkg: Path | None = None,
) -> None:
    """Run a smoke test as a subprocess; raise if exit != 0."""
    env = dict(os.environ)
    if src is not None:
        env["SRC"] = str(src)
    if pkg is not None:
        env["PKG"] = str(pkg)
    proc = subprocess.run(
        [
            sys.executable,
            "-c",
            "import os; SRC = os.environ.get('SRC'); PKG = os.environ.get('PKG'); "
            + smoke_test,
        ],
        env=env,
        capture_output=True,
        text=True,
    )
    if proc.returncode != 0:
        raise RuntimeError(
            f"Smoke test failed:\nstdout:\n{proc.stdout}\nstderr:\n{proc.stderr}"
        )


def run_topos_evaluate(
    target: Path,
    *,
    priority: str,
    recursive: bool,
    gitnexus_dir: Path | None = None,
) -> dict:
    """Run the current Topos classifier and return a CLI-shaped payload."""
    profile = Priority(priority)
    paths = sorted(target.rglob("*.py")) if recursive else [target]
    results = []
    for path in paths:
        result, dep_graph = classify_file(path, profile, gitnexus_dir)
        summary = result.summary()
        results.append(
            {
                "file": str(path),
                "lattice_element": summary.name,
                "lattice_symbol": summary.symbol,
                "dimensions": {
                    dim: value.name for dim, value in result.dimensions.items()
                },
                "scores": {dim: score * 100.0 for dim, score in result.scores.items()},
                "raw_metrics": dict(result.raw_metrics),
                "priority": profile.value,
                "coupling_available": dep_graph is not None,
            }
        )
    return {"results": results}


def baseline_summary_simple(payload: dict) -> dict:
    results = payload.get("results", [])
    if not results:
        return {"score": None, "lattice_element": "SLOP"}
    result = results[0]
    return {
        "lattice_element": result.get("lattice_element"),
        "lattice_symbol": result.get("lattice_symbol"),
        "simple_score": result.get("scores", {}).get("simple"),
        "raw_metrics": result.get("raw_metrics", {}),
        "priority": result.get("priority"),
    }


def baseline_summary_composable(payload: dict) -> dict:
    results = payload.get("results", [])
    if not results:
        return {"per_file": [], "lattice_elements": {}}

    per_file = []
    lattice_counts: dict[str, int] = {}
    composable_scores: list[float] = []
    for result in results:
        per_file.append(
            {
                "file": result.get("file"),
                "lattice_element": result.get("lattice_element"),
                "scores": result.get("scores", {}),
                "raw_metrics": result.get("raw_metrics", {}),
            }
        )
        lattice = result.get("lattice_element", "SLOP")
        lattice_counts[lattice] = lattice_counts.get(lattice, 0) + 1
        composable = result.get("scores", {}).get("composable")
        if isinstance(composable, (int, float)):
            composable_scores.append(float(composable))

    avg_composable = (
        sum(composable_scores) / len(composable_scores) if composable_scores else None
    )
    return {
        "per_file": per_file,
        "lattice_counts": lattice_counts,
        "avg_composable_score": avg_composable,
        "n_files": len(results),
    }


def gitnexus_available() -> bool:
    return shutil.which("gitnexus") is not None


def _run(cmd: list[str], *, cwd: Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(cmd, cwd=cwd, capture_output=True, text=True)


def stage_in_isolated_git_repo(src_dir: Path, dest_dir: Path) -> Path:
    """Copy ``src_dir`` into ``dest_dir`` as a standalone git repo.

    ``gitnexus analyze`` walks up to the nearest ``.git`` and indexes from
    there, so we give every composable baseline its own root. The staged copy
    is what gets perturbed; the original under ``corpus/composable/`` stays
    pristine.
    """
    if dest_dir.exists():
        shutil.rmtree(dest_dir)
    dest_dir.mkdir(parents=True)

    pkg_name = src_dir.name
    shutil.copytree(src_dir, dest_dir / pkg_name)

    proc = _run(["git", "init", "-q"], cwd=dest_dir)
    if proc.returncode != 0:
        raise RuntimeError(f"git init failed: {proc.stderr}")
    _run(["git", "config", "user.email", "topos-sensitivity@local"], cwd=dest_dir)
    _run(["git", "config", "user.name", "topos-sensitivity"], cwd=dest_dir)
    _run(["git", "add", "."], cwd=dest_dir)
    proc = _run(
        ["git", "commit", "-q", "-m", "stage for sensitivity benchmark"],
        cwd=dest_dir,
    )
    if proc.returncode != 0:
        raise RuntimeError(f"git commit failed: {proc.stderr}")

    return dest_dir / pkg_name


def run_gitnexus_analyze(target_dir: Path) -> None:
    proc = _run(["gitnexus", "analyze", "--force", "--skip-agents-md"], cwd=target_dir)
    if proc.returncode != 0:
        raise RuntimeError(
            f"gitnexus analyze failed in {target_dir}\n"
            f"stdout:\n{proc.stdout}\nstderr:\n{proc.stderr}"
        )
    if not (target_dir / ".gitnexus").exists():
        raise RuntimeError(
            f"gitnexus analyze completed but no .gitnexus in {target_dir}"
        )


def curate_simple() -> list[dict]:
    out_root = sensitivity_dir() / "corpus" / "self_contained"
    out_root.mkdir(parents=True, exist_ok=True)

    entries: list[dict] = []
    for cand in SIMPLE_CANDIDATES:
        print(f"[simple] preparing {cand.name} ({cand.package}=={cand.version})")
        extracted = fetch_extracted(cand.package, cand.version)
        source = locate_source(extracted, cand.relative_source)

        dest = out_root / f"{cand.name}.py"
        shutil.copy2(source, dest)

        smoke_check(cand.smoke_test, src=dest)
        payload = run_topos_evaluate(dest, priority="simple", recursive=False)
        baseline = baseline_summary_simple(payload)

        entries.append(
            {
                "name": cand.name,
                "package": cand.package,
                "version": cand.version,
                "source": str(dest.relative_to(repo_root())),
                "origin_relative": cand.relative_source,
                "smoke_test": cand.smoke_test,
                "baseline": baseline,
            }
        )
        print(
            f"  -> {baseline['lattice_element']} (score={baseline.get('simple_score')})"
        )

    (out_root / "manifest.json").write_text(
        json.dumps({"entries": entries}, indent=2), encoding="utf-8"
    )
    return entries


def curate_composable() -> list[dict]:
    out_root = sensitivity_dir() / "corpus" / "composable"
    staging_root = cache_dir() / "staging"
    out_root.mkdir(parents=True, exist_ok=True)
    staging_root.mkdir(parents=True, exist_ok=True)

    entries: list[dict] = []
    have_gitnexus = gitnexus_available()
    if not have_gitnexus:
        print(
            "[composable] gitnexus not on PATH — coupling baselines will be omitted. "
            "Install with: npm install -g gitnexus"
        )

    for cand in COMPOSABLE_CANDIDATES:
        print(f"[composable] preparing {cand.name} ({cand.package}=={cand.version})")
        extracted = fetch_extracted(cand.package, cand.version)
        pkg_src = locate_package_dir(extracted, cand.relative_package_dir)

        # Pristine canonical copy under corpus/. Drop ``tests/`` subdirs so
        # that test-side importers do not skew the package-level coupling
        # metrics — the corpus is the *library surface* of each package.
        canonical_pkg = out_root / cand.name / cand.relative_package_dir
        if canonical_pkg.exists():
            shutil.rmtree(canonical_pkg)
        canonical_pkg.parent.mkdir(parents=True, exist_ok=True)
        shutil.copytree(
            pkg_src,
            canonical_pkg,
            ignore=shutil.ignore_patterns("tests", "test_*"),
        )

        smoke_check(cand.smoke_test, pkg=canonical_pkg)

        baseline: dict = {"gitnexus_available": have_gitnexus}
        if have_gitnexus:
            staging_dir = staging_root / cand.name
            try:
                staged_pkg = stage_in_isolated_git_repo(canonical_pkg, staging_dir)
                run_gitnexus_analyze(staging_dir)
                gitnexus_dir = staging_dir / ".gitnexus"
                payload = run_topos_evaluate(
                    staged_pkg,
                    priority="composable",
                    recursive=True,
                    gitnexus_dir=gitnexus_dir,
                )
                baseline.update(baseline_summary_composable(payload))
                baseline["staging_dir"] = str(staging_dir.relative_to(repo_root()))
            except Exception as exc:  # noqa: BLE001 — surface the error in the manifest
                baseline["error"] = str(exc)

        entries.append(
            {
                "name": cand.name,
                "package": cand.package,
                "version": cand.version,
                "package_dir": str(canonical_pkg.relative_to(repo_root())),
                "smoke_test": cand.smoke_test,
                "baseline": baseline,
            }
        )
        print(
            "  -> "
            + (
                f"avg_composable_score={baseline.get('avg_composable_score')}"
                f" lattice_counts={baseline.get('lattice_counts')}"
                if have_gitnexus and "error" not in baseline
                else (
                    "(no coupling baseline: "
                    f"{baseline.get('error', 'gitnexus missing')})"
                )
            )
        )

    (out_root / "manifest.json").write_text(
        json.dumps({"entries": entries}, indent=2), encoding="utf-8"
    )
    return entries


def main() -> None:
    print("Curating SIMPLE corpus...")
    simple = curate_simple()
    print()
    print("Curating Composable corpus...")
    composable = curate_composable()

    print()
    print("=" * 60)
    print(f"Pinned {len(simple)} SIMPLE entries")
    for entry in simple:
        baseline = entry["baseline"]
        print(
            f"  {entry['name']:32} "
            f"{baseline.get('lattice_element', 'n/a'):16} "
            f"score={baseline.get('simple_score')}"
        )
    print()
    print(f"Pinned {len(composable)} composable entries")
    for entry in composable:
        baseline = entry["baseline"]
        print(
            f"  {entry['name']:32} "
            f"lattice_counts={baseline.get('lattice_counts', 'n/a')} "
            f"avg_composable={baseline.get('avg_composable_score')}"
        )


if __name__ == "__main__":
    main()
