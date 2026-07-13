"""Shared helpers for CLI evaluate / inspect — file discovery and formatting."""

from __future__ import annotations

import json
from pathlib import Path
from typing import TYPE_CHECKING

import click

from topos.graphs.ast.languages import language_file_suffixes
from topos.utils.discovery import collect_source_files

if TYPE_CHECKING:
    from topos.evaluation.characteristic_morphism import ClassificationResult

_DETAIL_FILE_LIMIT = 5


def collect_files(paths: tuple[str, ...], recursive: bool, language: str) -> list[Path]:
    """Collect source files for *language* from *paths* (files or directories)."""
    suffixes = tuple(language_file_suffixes(language))
    return collect_source_files(paths, suffixes=suffixes, recursive=recursive)


def run_classify_file(
    filepath: Path,
    *,
    priority: str,
    gitnexus_dir: str | None,
) -> ClassificationResult:
    """Classify one file using the same pipeline as MCP evaluate-file."""
    from topos.evaluation.policies import Priority
    from topos.mcp.evaluation import classify_file

    gdir = Path(gitnexus_dir).expanduser() if gitnexus_dir else None
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
        "priority": result.priority,
        "raw_metrics": result.raw_metrics,
        "entropy": entropy,
        "valid": result.is_parseable,
        "_result": result,
    }


_PILLAR_THRESHOLDS = {
    "simple": 60.0,
    "composable": 60.0,
    "secure": 70.0,
}


def _build_gauge(score: float, threshold: float, width: int = 10) -> str:
    """Build a mathematically aligned progress track."""
    score_idx = min(width, max(0, round((score / 100.0) * width)))
    threshold_idx = min(width, max(0, round((threshold / 100.0) * width)))

    # Track characters: use dimmer line
    track_char = "─"

    if score_idx == threshold_idx:
        color = "green" if score >= threshold else "red"
        bullet = click.style("◆", fg=color, bold=True)
        left = click.style(track_char * score_idx, dim=True)
        right = click.style(track_char * (width - score_idx), dim=True)
        return f"{left}{bullet}{right}"
    elif score_idx < threshold_idx:
        bullet = click.style("◆", fg="red", bold=True)
        pipe = click.style("│", fg="white", dim=True)
        left = click.style(track_char * score_idx, dim=True)
        middle = click.style(track_char * (threshold_idx - score_idx - 1), dim=True)
        right = click.style(track_char * (width - threshold_idx), dim=True)
        return f"{left}{bullet}{middle}{pipe}{right}"
    else:
        bullet = click.style("◆", fg="green", bold=True)
        pipe = click.style("│", fg="white", dim=True)
        left = click.style(track_char * threshold_idx, dim=True)
        middle = click.style(track_char * (score_idx - threshold_idx - 1), dim=True)
        right = click.style(track_char * (width - score_idx), dim=True)
        return f"{left}{pipe}{middle}{bullet}{right}"


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


def _format_file_row(
    rank_str: str, filepath: str, result: dict[str, object], max_file_len: int = 42
) -> str:
    # 1. File path truncation from left if it exceeds max_file_len
    file_path_str = str(filepath)
    if len(file_path_str) > max_file_len:
        file_path_str = "..." + file_path_str[-(max_file_len - 3) :]
    file_col = f"{file_path_str:<{max_file_len}}"

    # 2. Medal formatting
    symbol = result.get("lattice_symbol", "")
    element = result.get("lattice_element", "SLOP")
    medal_text = f"[{symbol} {element}]"
    if element == "IDEAL":
        medal_col = click.style(f"{medal_text:<16}", fg="green", bold=True)
    elif "COMPOSABLE" in element or "SECURE" in element or "SIMPLE" in element:
        if symbol == "🥈":
            medal_col = click.style(f"{medal_text:<16}", fg="yellow", bold=True)
        elif symbol == "🥉":
            medal_col = click.style(f"{medal_text:<16}", fg="cyan")
        else:
            medal_col = click.style(f"{medal_text:<16}", fg="red")
    else:
        medal_col = click.style(f"{medal_text:<16}", fg="red", dim=True)

    # 3. Score formatting (right aligned, padded with spaces)
    avg_score = _display_score(result)
    score_col = click.style(f"{avg_score:>3.0f}%", bold=True)

    # 4. Pillar scores with micro-gauges
    scores = result.get("scores", {})
    pillar_parts = []
    if isinstance(scores, dict):
        ordered_pillars = ["simple", "composable", "secure"]
        for p in ordered_pillars:
            if p in scores:
                p_score = float(scores[p])
                p_label = p[0].upper()  # 'S', 'C', 'S'
                gauge = _build_gauge(p_score, _PILLAR_THRESHOLDS[p], width=10)
                p_threshold = _PILLAR_THRESHOLDS[p]
                score_color = "green" if p_score >= p_threshold else "red"
                score_percent_str = click.style(f"{p_score:>3.0f}%", fg=score_color)

                label_str = click.style(f"{p_label}:", dim=True)
                bracket_left = click.style("[", dim=True)
                bracket_right = click.style("]", dim=True)

                gauge_str = (
                    f"{label_str}{score_percent_str} "
                    f"{bracket_left}{gauge}{bracket_right}"
                )
                pillar_parts.append(gauge_str)

    pillar_col = "  ".join(pillar_parts)

    return f"  {rank_str:<4}  {file_col}  {medal_col}   {score_col}    {pillar_col}"


def _output_file_details(results: list[dict[str, object]], verbose: bool) -> None:
    for result in results:
        symbol = result.get("lattice_symbol", "")
        element = result.get("lattice_element", "SLOP")
        medal_text = f"[{symbol} {element}]"

        if element == "IDEAL":
            medal_styled = click.style(medal_text, fg="green", bold=True)
        elif symbol == "🥈":
            medal_styled = click.style(medal_text, fg="yellow", bold=True)
        elif symbol == "🥉":
            medal_styled = click.style(medal_text, fg="cyan")
        else:
            medal_styled = click.style(medal_text, fg="red", dim=True)

        avg_score = _display_score(result)
        score_styled = click.style(f"{avg_score:.0f}%", bold=True)
        click.echo(f"  {result['file']}  {medal_styled}  {score_styled}")

        scores = result.get("scores", {})
        dimensions = result.get("dimensions", {})
        if isinstance(scores, dict):
            click.echo(click.style("    Pillar Progress vs Threshold (│)", dim=True))
            click.echo(click.style("    ────── ───────────────────────────", dim=True))
            for dim in ["simple", "composable", "secure"]:
                if dim in scores:
                    score = float(scores[dim])
                    threshold = _PILLAR_THRESHOLDS[dim]
                    status = "PASS" if score >= threshold else "FAIL"
                    status_styled = click.style(
                        f"{status:<4}", fg="green" if status == "PASS" else "red"
                    )
                    gauge = _build_gauge(score, threshold, width=20)

                    dim_label = f"    {dim:<10}"
                    bracket_l = click.style("[", dim=True)
                    bracket_r = click.style("]", dim=True)
                    score_str = click.style(
                        f"{score:>3.0f}%", fg="green" if status == "PASS" else "red"
                    )
                    req_str = click.style(f"(Req: {threshold:.0f}%)", dim=True)

                    click.echo(
                        f"{dim_label} {bracket_l}{gauge}{bracket_r}  "
                        f"{score_str}  {req_str}  {status_styled}"
                    )

        if not dimensions:
            click.echo(click.style("    ⊥ SLOP (parse failure)", fg="red", dim=True))

        if verbose:
            click.echo(click.style("    Raw Metrics:", dim=True))
            for key, value in result["raw_metrics"].items():
                click.echo(click.style(f"      {key}: {value:.3f}", dim=True))
            if "error" in result:
                click.echo(click.style(f"      Error: {result['error']}", fg="red"))
            _render_file_diagnostics(result)
        click.echo()


def _render_file_diagnostics(result: dict[str, object]) -> None:
    """Render per-file security findings + suggestions (verbose mode)."""
    from topos.cli.diagnostics import (
        render_security_findings,
        render_suggestions,
        render_verdict_line,
    )

    active = result.get("_active_findings") or []
    acknowledged = result.get("_acknowledged") or []
    verdict = result.get("_verdict")
    suggestions = result.get("_suggestions") or []
    render_security_findings(active, acknowledged, indent="    ")
    if verdict is not None:
        render_verdict_line(verdict, indent="    ")
    render_suggestions(suggestions, indent="    ")


def _lowest_hanging_fruit(
    results: list[dict[str, object]],
) -> list[dict[str, object]]:
    """Find files whose cheapest failing pillar is closest to passing.

    For each file we look at every *measured* pillar that fails its threshold,
    take the one with the smallest gap (the easiest win), and rank files
    ascending by that gap. Parse failures (no dimensions / SLOP) are skipped —
    they belong in "Needs attention", not here. Returns up to the top 5.
    """
    fruit: list[dict[str, object]] = []
    for result in results:
        scores = result.get("scores")
        dimensions = result.get("dimensions")
        if not isinstance(scores, dict) or not scores:
            continue
        # Skip parse failures — they have no measured dimensions to improve.
        if not isinstance(dimensions, dict) or not dimensions:
            continue

        best_gap: dict[str, object] | None = None
        for pillar, raw_score in scores.items():
            threshold = _PILLAR_THRESHOLDS.get(str(pillar))
            if threshold is None:
                continue
            score = float(raw_score)
            if score >= threshold:
                continue
            gap = threshold - score
            if best_gap is None or gap < float(best_gap["gap"]):
                best_gap = {
                    "pillar": str(pillar),
                    "score": score,
                    "threshold": threshold,
                    "gap": gap,
                }

        if best_gap is None:
            continue

        suggestion = _suggestion_for_pillar(result, str(best_gap["pillar"]))
        fruit.append(
            {
                "file": result["file"],
                "pillar": best_gap["pillar"],
                "score": best_gap["score"],
                "threshold": best_gap["threshold"],
                "gap": best_gap["gap"],
                "suggestion": suggestion,
            }
        )

    fruit.sort(key=lambda item: (float(item["gap"]), str(item["file"])))
    return fruit[:5]


def _suggestion_for_pillar(result: dict[str, object], pillar: str) -> str | None:
    """Return the most relevant suggestion message for *pillar*, or None.

    Prefers a "fix" (gate-failure) suggestion over an advisory "improve" one.
    """
    suggestions = result.get("_suggestions") or []
    matches = [s for s in suggestions if getattr(s, "pillar", None) == pillar]
    if not matches:
        return None
    matches.sort(key=lambda s: 0 if getattr(s, "severity", "") == "fix" else 1)
    return getattr(matches[0], "message", None)


def output_directory_average(results: list[dict[str, object]]) -> None:
    """Consolidated into output_text. This is now a backward compatible no-op."""
    pass


def output_overall(overall: dict[str, object]) -> None:
    """Consolidated into output_text. This is now a backward compatible no-op."""
    pass


def output_text(results: list[dict[str, object]], verbose: bool) -> None:
    """Output results as a compact summary, with detailed rows in verbose mode."""
    click.echo(
        click.style(
            f"Evaluated {len(results)} file{'s' if len(results) != 1 else ''}",
            bold=True,
        )
    )

    # Compute overall directory floor verdict
    from topos.evaluation.characteristic_morphism import CharacteristicMorphism

    classifier = CharacteristicMorphism()
    classification_results = [r["_result"] for r in results]
    overall = classifier.combine_dimensions(classification_results)

    if verbose or len(results) <= _DETAIL_FILE_LIMIT:
        click.echo()
        click.echo(click.style("Files", fg="cyan", bold=True))
        _output_file_details(results, verbose=verbose)
    else:
        pillars: set[str] = set()
        for result in results:
            scores = result["scores"]
            if isinstance(scores, dict):
                pillars.update(str(dim) for dim in scores)

        if pillars:
            click.echo()
            click.echo(click.style("Pillars", fg="cyan", bold=True))

            header_line = (
                f"  {'Pillar':<12} {'Status':<8} {'Avg Score':<11} "
                f"{'Min Score':<11} {'Failures':<10} "
                f"{'Progress vs Threshold (│)':<25}"
            )
            click.echo(click.style(header_line, bold=True))
            divider = (
                f"  {'──────':<12} {'──────':<8} {'─────────':<11} "
                f"{'─────────':<11} {'────────':<10} "
                f"{'─────────────────────────':<25}"
            )
            click.echo(click.style(divider, dim=True))

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

                floor_val = overall.get(pillar)
                is_passing = getattr(floor_val, "name", "SLOP") != "SLOP"
                status_text = "PASS" if is_passing else "FAIL"
                status_styled = click.style(
                    f"{status_text:<8}", fg="green" if is_passing else "red", bold=True
                )

                threshold = _PILLAR_THRESHOLDS.get(pillar, 60.0)
                gauge = _build_gauge(avg_score, threshold, width=20)

                click.echo(
                    f"  {pillar:<12} "
                    f"{status_styled} "
                    f"{avg_score:>9.0f}%   "
                    f"{min_score:>9.0f}%   "
                    f"{failures:>3d}/{len(results):<6} "
                    f"{click.style('[', dim=True)}{gauge}{click.style(']', dim=True)}"
                )

    average = sum(_display_score(result) for result in results) / len(results)
    avg_styled = click.style(f"{average:.0f}%", bold=True)
    click.echo(f"  Directory Average Score: {avg_styled} (Mean across all files)")

    from topos.core.omega import EvaluationValue, verdict_from_generators

    meet_val = verdict_from_generators(
        simple=overall.get("simple", EvaluationValue.SLOP) != EvaluationValue.SLOP,
        composable=overall.get("composable", EvaluationValue.SLOP)
        != EvaluationValue.SLOP,
        secure=overall.get("secure", EvaluationValue.SLOP) != EvaluationValue.SLOP,
    )

    meet_styled = click.style(
        f"{meet_val.symbol} {meet_val.name}",
        fg="green" if meet_val == EvaluationValue.IDEAL else "red",
        bold=True,
    )
    click.echo(f"  Directory Floor Verdict: {meet_styled} (Pointwise lattice meet)")
    click.echo(
        click.style(
            "  [INFO] The Floor Verdict is the minimum category-theoretic level "
            "achieved across all files.",
            dim=True,
        )
    )

    if not (verbose or len(results) <= _DETAIL_FILE_LIMIT):
        ranked = sorted(
            results,
            key=lambda result: (_display_score(result), result["file"]),
        )
        click.echo()
        click.echo(click.style("Needs attention", fg="cyan", bold=True))
        header_files = (
            f"  {'Rank':<4}  {'File':<42}  {'Verdict':<16}   "
            f"{'Score':<5}    {'Pillar Scores'}"
        )
        click.echo(click.style(header_files, bold=True))
        divider_files = (
            f"  {'────':<4}  "
            f"{'──────────────────────────────────────────':<42}  "
            f"{'───────':<16}   {'─────':<5}    {'─────────────'}"
        )
        click.echo(click.style(divider_files, dim=True))

        for idx, result in enumerate(ranked[:5], start=1):
            click.echo(_format_file_row(f"{idx}.", result["file"], result))

        best = max(
            results,
            key=lambda result: (_display_score(result), result["file"]),
        )
        click.echo()
        click.echo(click.style("Best file", fg="cyan", bold=True))
        click.echo(click.style(header_files, bold=True))
        click.echo(click.style(divider_files, dim=True))
        click.echo(_format_file_row("-", best["file"], best))

        _output_lowest_hanging_fruit(results)


def _output_lowest_hanging_fruit(results: list[dict[str, object]]) -> None:
    """Render the cheapest-win section: failing pillars closest to passing."""
    fruit = _lowest_hanging_fruit(results)
    click.echo()
    click.echo(click.style("Lowest-hanging fruit", fg="cyan", bold=True))
    if not fruit:
        click.echo(
            click.style(
                "  All measured pillars pass — no near-misses to fix.", dim=True
            )
        )
        return

    click.echo(
        click.style("  Smallest improvement that flips a failing pillar.", dim=True)
    )
    for idx, item in enumerate(fruit, start=1):
        pillar = str(item["pillar"])
        score = float(item["score"])
        threshold = float(item["threshold"])
        gap = float(item["gap"])

        file_str = str(item["file"])
        if len(file_str) > 42:
            file_str = "..." + file_str[-39:]

        gap_str = click.style(
            f"{pillar} {score:.0f}% → {threshold:.0f}% (+{gap:.0f} pts)",
            fg="yellow",
            bold=True,
        )
        click.echo(f"  {idx}.  {file_str}")
        click.echo(f"      {gap_str}")
        suggestion = item.get("suggestion")
        if suggestion:
            click.echo(click.style(f"      ↳ {suggestion}", dim=True))


def output_json(results: list[dict[str, object]]) -> None:
    """Output results as JSON."""
    from topos._version import __version__

    serialisable = [
        {k: v for k, v in r.items() if not k.startswith("_")} for r in results
    ]
    output = {
        "version": __version__,
        "results": serialisable,
    }
    click.echo(json.dumps(output, indent=2))
