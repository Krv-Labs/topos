"""Sweep coupling noise over the Composable corpus.

For each curated baseline package and each transform in ``noise.coupling``,
applies the transform at a grid of intensities, re-runs
``gitnexus analyze`` against an isolated git-init'd staging copy, runs
``topos evaluate -r --json --priority composable --gitnexus-dir <…>``
against the perturbed package, and records the resulting per-file metrics.

Writes:

- ``results/composable_sweep.json``: full machine-readable matrix.
- ``results/composable_sweep.md``:  per-baseline markdown tables.
"""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[3]
SENSITIVITY_DIR = REPO_ROOT / "demos" / "sensitivity"
sys.path.insert(0, str(SENSITIVITY_DIR))

from noise import coupling  # noqa: E402

INTENSITIES: tuple[int, ...] = (0, 1, 2, 4, 8)


def topos_env() -> dict[str, str]:
    env = dict(os.environ)
    src_root = REPO_ROOT / "src"
    existing = env.get("PYTHONPATH", "")
    env["PYTHONPATH"] = (
        str(src_root) if not existing else f"{src_root}{os.pathsep}{existing}"
    )
    return env


def _run(cmd: list[str], cwd: Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(cmd, cwd=cwd, text=True, capture_output=True, env=topos_env())


def stage_in_git_repo(src_pkg: Path, dest_dir: Path) -> Path:
    """Copy the canonical package into a fresh git-init'd ``dest_dir``."""
    if dest_dir.exists():
        shutil.rmtree(dest_dir)
    dest_dir.mkdir(parents=True)
    pkg_name = src_pkg.name
    shutil.copytree(src_pkg, dest_dir / pkg_name)

    proc = _run(["git", "init", "-q"], cwd=dest_dir)
    if proc.returncode != 0:
        raise RuntimeError(f"git init failed in {dest_dir}: {proc.stderr}")
    _run(["git", "config", "user.email", "topos-sensitivity@local"], cwd=dest_dir)
    _run(["git", "config", "user.name", "topos-sensitivity"], cwd=dest_dir)
    _run(["git", "add", "."], cwd=dest_dir)
    proc = _run(["git", "commit", "-q", "-m", "stage"], cwd=dest_dir)
    if proc.returncode != 0:
        raise RuntimeError(f"git commit failed in {dest_dir}: {proc.stderr}")
    return dest_dir / pkg_name


def gitnexus_analyze(repo_dir: Path) -> None:
    proc = _run(["gitnexus", "analyze", "--force", "--skip-agents-md"], cwd=repo_dir)
    if proc.returncode != 0:
        raise RuntimeError(
            f"gitnexus analyze failed in {repo_dir}\n"
            f"stdout:\n{proc.stdout}\nstderr:\n{proc.stderr}"
        )
    if not (repo_dir / ".gitnexus").exists():
        raise RuntimeError(f"No .gitnexus generated in {repo_dir}")


def topos_evaluate(pkg_dir: Path, *, gitnexus_dir: Path) -> list[dict]:
    """Run ``topos evaluate -r --json --priority composable`` and return results."""
    command = [
        sys.executable,
        "-m",
        "topos.main",
        "evaluate",
        str(pkg_dir),
        "-r",
        "--json",
        "--priority",
        "composable",
        "--gitnexus-dir",
        str(gitnexus_dir),
    ]
    proc = subprocess.run(
        command, cwd=REPO_ROOT, env=topos_env(), text=True, capture_output=True
    )
    if proc.returncode != 0:
        raise RuntimeError(
            f"topos evaluate failed for {pkg_dir}\n"
            f"stdout:\n{proc.stdout}\nstderr:\n{proc.stderr}"
        )
    text = proc.stdout
    marker = "\n\nOverall:"
    if marker in text:
        text = text.split(marker, maxsplit=1)[0]
    payload = json.loads(text.strip())
    return payload.get("results", [])


def summarize_results(results: list[dict]) -> dict:
    """Aggregate per-file results into a single coupling summary."""
    lattice_counts: dict[str, int] = {}
    coupling_scores: list[float] = []
    raw_coupling: list[float] = []
    raw_instability: list[float] = []
    for r in results:
        lattice = r.get("lattice_element", "BROKEN")
        lattice_counts[lattice] = lattice_counts.get(lattice, 0) + 1
        coupling = r.get("scores", {}).get("coupling")
        if isinstance(coupling, (int, float)):
            coupling_scores.append(float(coupling))
        raw = r.get("raw_metrics", {})
        if isinstance(raw.get("depgraph.coupling"), (int, float)):
            raw_coupling.append(float(raw["depgraph.coupling"]))
        if isinstance(raw.get("depgraph.instability"), (int, float)):
            raw_instability.append(float(raw["depgraph.instability"]))

    def _mean(xs: list[float]) -> float | None:
        return sum(xs) / len(xs) if xs else None

    return {
        "n_files": len(results),
        "lattice_counts": lattice_counts,
        "avg_coupling_score": _mean(coupling_scores),
        "avg_raw_coupling": _mean(raw_coupling),
        "avg_raw_instability": _mean(raw_instability),
    }


def load_manifest() -> list[dict]:
    path = SENSITIVITY_DIR / "corpus" / "composable" / "manifest.json"
    if not path.exists():
        raise FileNotFoundError(
            f"Composable manifest not found at {path}. "
            "Run demos/sensitivity/curate.py first."
        )
    return json.loads(path.read_text(encoding="utf-8"))["entries"]


def sweep_baseline(entry: dict, staging_root: Path) -> dict:
    name = entry["name"]
    canonical_pkg = REPO_ROOT / entry["package_dir"]

    per_transform: dict[str, list[dict]] = {}
    for transform_name, transform_fn in coupling.TRANSFORMS.items():
        rows: list[dict] = []
        for intensity in INTENSITIES:
            print(f"  {transform_name} @ intensity={intensity}", flush=True)
            row: dict = {"intensity": intensity}
            staging_dir = staging_root / name / f"{transform_name}_{intensity}"
            try:
                staged_pkg = stage_in_git_repo(canonical_pkg, staging_dir)
                transform_fn(staged_pkg, intensity)
                gitnexus_analyze(staging_dir)
                results = topos_evaluate(
                    staged_pkg, gitnexus_dir=staging_dir / ".gitnexus"
                )
                row["summary"] = summarize_results(results)
            except Exception as exc:  # noqa: BLE001
                row["error"] = str(exc)
            rows.append(row)
        per_transform[transform_name] = rows

    return {
        "name": name,
        "package_dir": entry["package_dir"],
        "package": entry.get("package"),
        "version": entry.get("version"),
        "transforms": per_transform,
    }


def format_markdown(sweep: list[dict]) -> str:
    lines: list[str] = [
        "# Composable Sensitivity Sweep",
        "",
        "Each table reports the package-level coupling summary under "
        "increasing intensity of a single coupling noise transform. Cells "
        "show `avg_coupling_score / avg_raw_coupling / avg_raw_instability`. "
        "Lattice counts and per-file detail are in the JSON artifact.",
        "",
    ]

    for record in sweep:
        lines.append(f"## `{record['name']}`")
        lines.append("")
        lines.append(f"Package: `{record['package_dir']}`")
        if record.get("package"):
            lines.append(f"Provenance: `{record['package']}=={record['version']}`")
        lines.append("")

        transforms = list(record["transforms"].keys())
        header = "| intensity | " + " | ".join(transforms) + " |"
        sep = "| --- | " + " | ".join("---" for _ in transforms) + " |"
        lines.append(header)
        lines.append(sep)

        for idx, intensity in enumerate(INTENSITIES):
            cells = [str(intensity)]
            for transform in transforms:
                cell = record["transforms"][transform][idx]
                if "error" in cell:
                    cells.append("`error`")
                    continue
                summary = cell.get("summary", {})
                score = summary.get("avg_coupling_score")
                coup = summary.get("avg_raw_coupling")
                inst = summary.get("avg_raw_instability")
                cells.append(
                    f"{_fmt(score, '.1f')} / {_fmt(coup, '.2f')} / {_fmt(inst, '.2f')}"
                )
            lines.append("| " + " | ".join(cells) + " |")
        lines.append("")

        for transform in transforms:
            lines.append(f"### `{transform}` — lattice counts")
            lines.append("")
            counts_per_intensity = []
            keys: set[str] = set()
            for cell in record["transforms"][transform]:
                summary = cell.get("summary", {})
                lc = summary.get("lattice_counts", {})
                counts_per_intensity.append(lc)
                keys.update(lc.keys())
            ordered_keys = sorted(keys)
            header = "| intensity | " + " | ".join(ordered_keys) + " |"
            sep = "| --- | " + " | ".join("---" for _ in ordered_keys) + " |"
            lines.append(header)
            lines.append(sep)
            for idx, intensity in enumerate(INTENSITIES):
                cells = [str(intensity)]
                lc = counts_per_intensity[idx]
                for key in ordered_keys:
                    cells.append(str(lc.get(key, 0)))
                lines.append("| " + " | ".join(cells) + " |")
            lines.append("")

    return "\n".join(lines)


def _fmt(value, spec: str) -> str:
    if isinstance(value, (int, float)):
        return f"{value:{spec}}"
    return "?"


def main() -> None:
    entries = load_manifest()
    staging_root = SENSITIVITY_DIR / ".cache" / "sweep_staging"

    sweep: list[dict] = []
    for entry in entries:
        print(f"[coupling] sweeping {entry['name']}")
        sweep.append(sweep_baseline(entry, staging_root))

    results_dir = SENSITIVITY_DIR / "results"
    results_dir.mkdir(parents=True, exist_ok=True)
    (results_dir / "composable_sweep.json").write_text(
        json.dumps({"intensities": list(INTENSITIES), "baselines": sweep}, indent=2),
        encoding="utf-8",
    )
    (results_dir / "composable_sweep.md").write_text(
        format_markdown(sweep), encoding="utf-8"
    )
    print()
    print(f"Wrote {results_dir / 'composable_sweep.json'}")
    print(f"Wrote {results_dir / 'composable_sweep.md'}")


if __name__ == "__main__":
    main()
