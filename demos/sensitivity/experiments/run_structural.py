"""Sweep structural noise over the SIMPLE corpus.

For each curated baseline file and each transform in ``noise.structural``,
applies the transform at a grid of intensities, runs the current Topos
classifier with ``priority=simple`` on the perturbed source, and records the
resulting score / lattice element / raw metrics. Also records the
normalized AST edit distance from the intensity-zero baseline so the
score-vs-edit-distance ratio can be inspected later.

Writes:

- ``results/self_contained_sweep.json``: full machine-readable matrix.
- ``results/self_contained_sweep.md``:  per-baseline markdown tables.
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

from noise import structural  # noqa: E402

INTENSITIES: tuple[int, ...] = (0, 1, 2, 4, 8, 16)


def evaluate(filepath: Path) -> dict:
    """Run the current classifier with ``priority=simple`` on a single file."""
    from topos.evaluation.policies.base import Priority
    from topos.mcp.evaluation import classify_file

    result, dep_graph = classify_file(filepath, Priority.SIMPLE, gitnexus_dir=None)
    summary = result.summary()
    return {
        "lattice_element": summary.name,
        "lattice_symbol": summary.symbol,
        "scores": {dim: score * 100.0 for dim, score in result.scores.items()},
        "dimensions": {dim: value.name for dim, value in result.dimensions.items()},
        "raw_metrics": dict(result.raw_metrics),
        "coupling_available": dep_graph is not None,
        "priority": Priority.SIMPLE.value,
    }


# Cap Wagner-Fischer DP at ~3k nodes (~36 MB int matrix) — above that we fall
# back to the cheap node-count delta proxy. This keeps the structural sweep
# affordable on the large baselines (tabulate has ~25k AST nodes).
MAX_AST_DISTANCE_NODES = 3_000


def _count_ast_nodes(source: str) -> int:
    try:
        return sum(1 for _ in ast.walk(ast.parse(source)))
    except SyntaxError:
        return 0


def tree_drift(source_a: str, source_b: str) -> dict:
    """Return a cheap structural drift summary between two source strings.

    Always reports node-count delta. When both trees are small (≤
    ``MAX_AST_DISTANCE_NODES`` nodes) also computes the normalized
    Wagner-Fischer AST edit distance. Anti-gaming inspection only needs
    "did the tree change?", which the cheap delta answers reliably for
    every cell.
    """
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
    path = SENSITIVITY_DIR / "corpus" / "self_contained" / "manifest.json"
    if not path.exists():
        raise FileNotFoundError(
            f"Self-contained manifest not found at {path}. "
            "Run demos/sensitivity/curate.py first."
        )
    return json.loads(path.read_text(encoding="utf-8"))["entries"]


def run_cell(
    *, baseline_source: str, transform_fn, intensity: int
) -> tuple[dict | None, dict | None, str | None]:
    """Apply transform, write to a tmp file, evaluate. Return result + drift.

    Returns ``(None, None, error_message)`` if the transform fails.
    """
    try:
        perturbed = transform_fn(baseline_source, intensity)
    except Exception as exc:  # noqa: BLE001 — surface in matrix
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
    baseline_source = structural.baseline(raw_source)

    per_transform: dict[str, list[dict]] = {}
    for transform_name, transform_fn in structural.TRANSFORMS.items():
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
                row["simple_score"] = result.get("scores", {}).get("simple")
                row["raw_metrics"] = result.get("raw_metrics", {})
                row["drift"] = drift
            rows.append(row)
        per_transform[transform_name] = rows

    return {
        "name": entry["name"],
        "source": entry["source"],
        "package": entry.get("package"),
        "version": entry.get("version"),
        "transforms": per_transform,
    }


def format_markdown(sweep: list[dict]) -> str:
    lines: list[str] = [
        "# SIMPLE Sensitivity Sweep",
        "",
        "Each table reports the score and lattice element of a baseline file "
        "under increasing intensity of a single structural noise transform. "
        "`Δn` is the AST-node delta from the intensity-0 baseline — the "
        "cheap anti-gaming proxy described in the runner. Normalized "
        "Wagner-Fischer edit distance is also recorded in the JSON artifact "
        "for baselines under 3,000 nodes.",
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
                score = cell.get("simple_score")
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
        print(f"[structural] sweeping {entry['name']}")
        sweep.append(sweep_baseline(entry))

    results_dir = SENSITIVITY_DIR / "results"
    results_dir.mkdir(parents=True, exist_ok=True)
    (results_dir / "self_contained_sweep.json").write_text(
        json.dumps({"intensities": list(INTENSITIES), "baselines": sweep}, indent=2),
        encoding="utf-8",
    )
    (results_dir / "self_contained_sweep.md").write_text(
        format_markdown(sweep), encoding="utf-8"
    )
    print()
    print(f"Wrote {results_dir / 'self_contained_sweep.json'}")
    print(f"Wrote {results_dir / 'self_contained_sweep.md'}")


if __name__ == "__main__":
    main()
