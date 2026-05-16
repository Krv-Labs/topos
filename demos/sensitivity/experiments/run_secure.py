"""Sweep SECURE-pillar noise over the SIMPLE corpus.

For each curated baseline file and each transform in ``noise.secure``,
applies the transform at a grid of intensities, runs the Topos classifier
with ``priority=secure``, and records lattice verdicts plus all three
generator scores (SIMPLE, COMPOSABLE, SECURE).

Writes:

- ``results/secure_sweep.json``: full machine-readable matrix.
- ``results/secure_sweep.md``: per-baseline markdown tables.
"""

from __future__ import annotations

import ast
import json
import sys
import tempfile
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[3]
SENSITIVITY_DIR = REPO_ROOT / "demos" / "sensitivity"
sys.path.insert(0, str(SENSITIVITY_DIR))

from noise import secure  # noqa: E402

PILLAR = "secure"
INTENSITIES: tuple[int, ...] = (0, 1, 2, 4, 8, 16)


def evaluate(filepath: Path) -> dict:
    """Classify a single file; return all three pillar scores and dimensions."""
    from topos.evaluation.policies.base import Priority
    from topos.mcp.evaluation import classify_file

    result, dep_graph = classify_file(filepath, Priority.SECURE, gitnexus_dir=None)
    summary = result.summary()
    return {
        "lattice_element": summary.name,
        "lattice_symbol": summary.symbol,
        "scores": {dim: score * 100.0 for dim, score in result.scores.items()},
        "dimensions": {dim: value.name for dim, value in result.dimensions.items()},
        "raw_metrics": dict(result.raw_metrics),
        "coupling_available": dep_graph is not None,
        "priority": Priority.SECURE.value,
    }


MAX_AST_DISTANCE_NODES = 3_000


def _count_ast_nodes(source: str) -> int:
    try:
        return sum(1 for _ in ast.walk(ast.parse(source)))
    except SyntaxError:
        return 0


def tree_drift(source_a: str, source_b: str) -> dict:
    """Cheap structural drift summary between two source strings."""
    nodes_a = _count_ast_nodes(source_a)
    nodes_b = _count_ast_nodes(source_b)
    drift = {
        "nodes_baseline": nodes_a,
        "nodes_perturbed": nodes_b,
        "nodes_delta": nodes_b - nodes_a,
    }

    if nodes_a <= MAX_AST_DISTANCE_NODES and nodes_b <= MAX_AST_DISTANCE_NODES:
        from topos.core.morphism import ProgramMorphism
        from topos.functors.profunctors.ast.compare import calculate_ast_distance

        a = ProgramMorphism(source=source_a, language="python")
        b = ProgramMorphism(source=source_b, language="python")
        if a.ast is not None and b.ast is not None:
            drift["ast_edit_distance"] = calculate_ast_distance(
                a.ast, b.ast
            ).normalized_distance

    return drift


def load_manifest() -> list[dict]:
    path = SENSITIVITY_DIR / "corpus" / "simple" / "manifest.json"
    if not path.exists():
        raise FileNotFoundError(
            f"SIMPLE corpus manifest not found at {path}. "
            "Run demos/sensitivity/curate.py first."
        )
    return json.loads(path.read_text(encoding="utf-8"))["entries"]


def run_cell(
    *, baseline_source: str, transform_fn, intensity: int
) -> tuple[dict | None, dict | None, str | None]:
    try:
        perturbed = transform_fn(baseline_source, intensity)
    except Exception as exc:  # noqa: BLE001
        return None, None, f"transform error: {exc}"

    try:
        ast.parse(perturbed)
    except SyntaxError as exc:
        return None, None, f"parse error: {exc}"

    with tempfile.NamedTemporaryFile(
        "w", suffix=".py", encoding="utf-8", delete=False
    ) as tmp:
        tmp.write(perturbed)
        tmp_path = Path(tmp.name)

    try:
        result = evaluate(tmp_path)
    except RuntimeError as exc:
        return None, None, f"evaluate error: {exc}"
    finally:
        tmp_path.unlink(missing_ok=True)

    drift = tree_drift(baseline_source, perturbed)
    return result, drift, None


def sweep_baseline(entry: dict) -> dict:
    source_path = REPO_ROOT / entry["source"]
    raw_source = source_path.read_text(encoding="utf-8")
    baseline_source = secure.baseline(raw_source)

    per_transform: dict[str, list[dict]] = {}
    for transform_name, transform_fn in secure.TRANSFORMS.items():
        rows: list[dict] = []
        for intensity in INTENSITIES:
            print(f"  {transform_name} @ intensity={intensity}", flush=True)
            result, drift, error = run_cell(
                baseline_source=baseline_source,
                transform_fn=transform_fn,
                intensity=intensity,
            )
            row: dict = {"intensity": intensity}
            if error is not None:
                row["error"] = error
            else:
                row["lattice_element"] = result.get("lattice_element")
                row["lattice_symbol"] = result.get("lattice_symbol")
                row["scores"] = result.get("scores", {})
                row["dimensions"] = result.get("dimensions", {})
                row["raw_metrics"] = result.get("raw_metrics", {})
                row["drift"] = drift
            rows.append(row)
        per_transform[transform_name] = rows

    return {
        "name": entry["name"],
        "source": entry["source"],
        "package": entry.get("package"),
        "version": entry.get("version"),
        "pillar": PILLAR,
        "transforms": per_transform,
    }


def format_markdown(sweep: list[dict]) -> str:
    lines: list[str] = [
        "# SECURE pillar sensitivity sweep",
        "",
        "Each table reports SECURE generator score and overall lattice verdict "
        "under increasing intensity of a single noise transform. "
        "`Δn` is the AST-node delta from the intensity-0 baseline. "
        "JSON artifacts also record SIMPLE and COMPOSABLE scores per cell.",
        "",
    ]

    for record in sweep:
        lines.append(f"## `{record['name']}`")
        lines.append("")
        lines.append(f"Source: `{record['source']}`")
        if record.get("package"):
            lines.append(f"Provenance: `{record['package']}=={record['version']}`")
        lines.append("")

        transforms = list(record["transforms"].keys())
        header = "| intensity | " + " | ".join(transforms) + " |"
        sep = "| --- | " + " | ".join("---" for _ in transforms) + " |"
        lines.append(header)
        lines.append(sep)

        for idx, intensity in enumerate(INTENSITIES):
            row_cells: list[str] = [str(intensity)]
            for transform in transforms:
                rows = record["transforms"][transform]
                cell = rows[idx]
                if "error" in cell:
                    row_cells.append("`error`")
                    continue
                scores = cell.get("scores", {}) or {}
                score = scores.get("secure")
                lattice = cell.get("lattice_symbol") or ""
                drift = cell.get("drift", {}) or {}
                delta_nodes = drift.get("nodes_delta")
                score_str = f"{score:.1f}" if isinstance(score, (int, float)) else "?"
                delta_str = (
                    f"Δn={delta_nodes:+d}" if isinstance(delta_nodes, int) else "Δn=?"
                )
                row_cells.append(f"{score_str} {lattice} ({delta_str})")
            lines.append("| " + " | ".join(row_cells) + " |")

        lines.append("")

    return "\n".join(lines)


def main() -> None:
    entries = load_manifest()
    sweep: list[dict] = []
    for entry in entries:
        print(f"[secure] sweeping {entry['name']}")
        sweep.append(sweep_baseline(entry))

    results_dir = SENSITIVITY_DIR / "results"
    results_dir.mkdir(parents=True, exist_ok=True)
    payload = {
        "pillar": PILLAR,
        "intensities": list(INTENSITIES),
        "baselines": sweep,
    }
    (results_dir / "secure_sweep.json").write_text(
        json.dumps(payload, indent=2),
        encoding="utf-8",
    )
    (results_dir / "secure_sweep.md").write_text(
        format_markdown(sweep), encoding="utf-8"
    )
    print()
    print(f"Wrote {results_dir / 'secure_sweep.json'}")
    print(f"Wrote {results_dir / 'secure_sweep.md'}")


if __name__ == "__main__":
    main()
