"""Shared helpers for CLI evaluate / inspect — file discovery and formatting."""

from __future__ import annotations

import json
from pathlib import Path

import click

from topos import __version__
from topos.evaluation.characteristic_morphism import ClassificationResult
from topos.graphs.ast.dispatch import language_file_suffixes
from topos.mcp.evaluation import classify_file


def collect_files(paths: tuple[str, ...], recursive: bool, language: str) -> list[Path]:
    """Collect source files for *language* from *paths* (files or directories)."""
    suffixes = language_file_suffixes(language)
    files: set[Path] = set()

    for path_str in paths:
        path = Path(path_str)

        if path.is_file():
            if path.suffix in suffixes:
                files.add(path)
        elif path.is_dir():
            for suffix in suffixes:
                pattern = f"**/*{suffix}" if recursive else f"*{suffix}"
                files.update(path.glob(pattern))

    return sorted(files)


def run_classify_file(
    filepath: Path,
    *,
    priority: str,
    gitnexus_dir: str | None,
) -> ClassificationResult:
    """Classify one file using the same pipeline as MCP evaluate-file."""
    gdir = Path(gitnexus_dir).expanduser() if gitnexus_dir else None
    from topos.evaluation.policies import Priority

    result, _deps = classify_file(filepath, Priority(priority), gdir)
    return result


def result_to_row(filepath: Path, result: ClassificationResult) -> dict[str, object]:
    """Shape a :class:`ClassificationResult` for text/JSON CLI output."""
    summary = result.summary()
    entropy = result.raw_metrics.get("ast.entropy", 0.0)
    return {
        "file": str(filepath),
        "is_parseable": result.is_parseable,
        "lattice_element": summary.name,
        "lattice_symbol": summary.symbol,
        "dimensions": {dim: val.name for dim, val in result.dimensions.items()},
        "dimension_symbols": {
            dim: val.symbol for dim, val in result.dimensions.items()
        },
        "scores": {dim: round(s * 100.0, 1) for dim, s in result.scores.items()},
        "priority": result.priority.value,
        "raw_metrics": result.raw_metrics,
        "entropy": entropy,
        "valid": result.is_parseable,
        "_result": result,
    }


def output_text(results: list[dict[str, object]], verbose: bool) -> None:
    """Output results as formatted text."""
    for result in results:
        click.echo(result["file"])
        dimensions = result["dimensions"]
        for dim, name in dimensions.items():
            sym = result["dimension_symbols"].get(dim, "")
            score = result["scores"].get(dim)
            score_str = f"  [{score:.0f}%]" if score is not None else ""
            click.echo(f"  {dim}: {sym} {name}{score_str}")
        if not dimensions:
            click.echo("  ⊥ SLOP (parse failure)")

        if verbose:
            for key, value in result["raw_metrics"].items():
                click.echo(f"    {key}: {value:.3f}")
            if "error" in result:
                click.echo(f"    Error: {result['error']}")


def output_json(results: list[dict[str, object]]) -> None:
    """Output results as JSON."""
    serialisable = [{k: v for k, v in r.items() if k != "_result"} for r in results]
    output = {
        "version": __version__,
        "results": serialisable,
    }
    click.echo(json.dumps(output, indent=2))
