"""Shared helpers for CLI evaluate / inspect — file discovery and formatting."""

from __future__ import annotations

import json
from pathlib import Path

import click

from topos import __version__
from topos.evaluation.characteristic_morphism import ClassificationResult
from topos.graphs.ast.dispatch import language_file_suffixes
from topos.mcp.evaluation import classify_file

_DETAIL_FILE_LIMIT = 5


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


def _score_bar(score: float, width: int = 10) -> str:
    filled = min(width, max(0, round((score / 100.0) * width)))
    return "█" * filled + "░" * (width - filled)


def _display_score(result: dict[str, object]) -> float:
    scores = result["scores"]
    if not isinstance(scores, dict) or not scores:
        return 0.0
    values = [float(value) for value in scores.values()]
    return sum(values) / len(values)


def _pillar_order(pillars: set[str]) -> list[str]:
    preferred = ["simple", "composable", "secure"]
    ordered = [pillar for pillar in preferred if pillar in pillars]
    ordered.extend(sorted(pillars - set(preferred)))
    return ordered


def _format_file_summary(result: dict[str, object]) -> str:
    scores = result["scores"]
    score_parts = []
    if isinstance(scores, dict):
        score_parts = [f"{dim} {float(score):.0f}" for dim, score in scores.items()]
    score_text = "  " + "  ".join(score_parts) if score_parts else ""
    medal = _format_medal(result)
    return f"{result['file']} {medal}  {_display_score(result):.0f}%{score_text}"


def _format_medal(result: dict[str, object]) -> str:
    symbol = result.get("lattice_symbol", "")
    element = result.get("lattice_element", "SLOP")
    return f"[{symbol} {element}]"


def _output_file_details(results: list[dict[str, object]], verbose: bool) -> None:
    for result in results:
        click.echo(f"{result['file']} {_format_medal(result)}")
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


def output_directory_average(results: list[dict[str, object]]) -> None:
    """Output average file scores so the floor verdict is not mistaken for average."""
    click.echo()
    click.echo("Directory Average Score")
    if not results:
        click.echo("  ⊥ no evaluable files")
        return

    average = sum(_display_score(result) for result in results) / len(results)
    click.echo(f"  mean file score {average:.0f}%  {_score_bar(average, 3)}")

    pillars: set[str] = set()
    for result in results:
        scores = result["scores"]
        if isinstance(scores, dict):
            pillars.update(str(dim) for dim in scores)

    for pillar in _pillar_order(pillars):
        pillar_scores = [
            float(result["scores"][pillar])
            for result in results
            if isinstance(result["scores"], dict) and pillar in result["scores"]
        ]
        if not pillar_scores:
            continue
        avg_score = sum(pillar_scores) / len(pillar_scores)
        click.echo(f"  {pillar:<10} avg {avg_score:>3.0f}%  {_score_bar(avg_score, 3)}")


def output_overall(overall: dict[str, object]) -> None:
    """Output the rolled-up floor pillar verdicts."""
    click.echo()
    click.echo("Directory Floor Verdict")
    if not overall:
        click.echo("  ⊥ no evaluable dimensions")
        return

    passed = sum(
        1 for value in overall.values() if getattr(value, "name", "") != "SLOP"
    )
    total = len(overall)
    symbol = "✅" if passed == total else "❌"
    pass_rate = passed / total * 100
    click.echo(
        f"  {symbol} {passed}/{total} pillars passing  {_score_bar(pass_rate, 3)}"
    )

    for dim in _pillar_order(set(overall)):
        value = overall[dim]
        click.echo(f"  {dim:<10} {value}")


def output_text(results: list[dict[str, object]], verbose: bool) -> None:
    """Output results as a compact summary, with detailed rows in verbose mode."""
    click.echo(f"Evaluated {len(results)} file{'s' if len(results) != 1 else ''}")

    if verbose or len(results) <= _DETAIL_FILE_LIMIT:
        click.echo()
        click.echo("Files")
        _output_file_details(results, verbose=verbose)
        return

    pillars: set[str] = set()
    for result in results:
        scores = result["scores"]
        if isinstance(scores, dict):
            pillars.update(str(dim) for dim in scores)

    if pillars:
        click.echo()
        click.echo("Pillars")
        for pillar in _pillar_order(pillars):
            pillar_scores = [
                float(result["scores"][pillar])
                for result in results
                if isinstance(result["scores"], dict) and pillar in result["scores"]
            ]
            dimensions = [
                result["dimensions"].get(pillar)
                for result in results
                if isinstance(result["dimensions"], dict)
            ]
            avg_score = sum(pillar_scores) / len(pillar_scores)
            min_score = min(pillar_scores)
            failures = sum(1 for value in dimensions if value == "SLOP")
            click.echo(
                f"  {pillar:<10} avg {avg_score:>3.0f}%  "
                f"min {min_score:>3.0f}%  fail {failures:<3} "
                f"{_score_bar(avg_score)}"
            )

    ranked = sorted(
        results,
        key=lambda result: (_display_score(result), result["file"]),
    )
    click.echo()
    click.echo("Needs attention")
    for idx, result in enumerate(ranked[:5], start=1):
        click.echo(f"  {idx}. {_format_file_summary(result)}")

    best = max(results, key=lambda result: (_display_score(result), result["file"]))
    click.echo()
    click.echo("Best file")
    click.echo(f"  {_format_file_summary(best)}")

    if verbose:
        click.echo()
        click.echo("Files")
        _output_file_details(results, verbose=True)


def output_json(results: list[dict[str, object]]) -> None:
    """Output results as JSON."""
    serialisable = [{k: v for k, v in r.items() if k != "_result"} for r in results]
    output = {
        "version": __version__,
        "results": serialisable,
    }
    click.echo(json.dumps(output, indent=2))
