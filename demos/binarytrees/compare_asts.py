"""Cross-language UAST comparison runner for the binarytrees benchmark."""

from __future__ import annotations

import json
from dataclasses import asdict
from pathlib import Path

from topos.graphs.ast.dispatch import parse_source
from topos.metrics.uast import compare_uast

LANGUAGES = {
    "python": "binarytrees.py",
    "rust": "binarytrees.rs",
    "javascript": "binarytrees.js",
    "cpp": "binarytrees.cpp",
}


def _load_uasts(src_dir: Path) -> dict[str, object]:
    uasts: dict[str, object] = {}
    for language, filename in LANGUAGES.items():
        path = src_dir / filename
        source = path.read_text(encoding="utf-8")
        result = parse_source(source=source, language=language, file=str(path))
        uasts[language] = result.uast_root
    return uasts


def _pairwise_report(uasts: dict[str, object]) -> dict[str, dict[str, dict]]:
    report: dict[str, dict[str, dict]] = {}
    for src_lang, src_root in uasts.items():
        report[src_lang] = {}
        for tgt_lang, tgt_root in uasts.items():
            comparison = compare_uast(src_root, tgt_root, include_unknown=False)
            report[src_lang][tgt_lang] = {
                "kind_distance": comparison.kind_distance,
                "edit_distance": {
                    "raw": comparison.edit_distance.raw_distance,
                    "normalized": comparison.edit_distance.normalized_distance,
                    "operations": comparison.edit_distance.operations,
                },
                "control_flow_delta": comparison.control_flow_delta,
                "summary_delta": comparison.summary_delta,
                "source_summary": asdict(comparison.source_summary),
                "target_summary": asdict(comparison.target_summary),
                "detects_difference": comparison.detects_difference,
            }
    return report


def _format_matrix(report: dict[str, dict[str, dict]], metric: str) -> str:
    languages = list(report.keys())
    header = "| | " + " | ".join(languages) + " |"
    separator = "| --- | " + " | ".join("---" for _ in languages) + " |"
    rows = [header, separator]
    for src in languages:
        cells = []
        for tgt in languages:
            value = report[src][tgt][metric]
            if isinstance(value, dict):
                value = value.get("normalized", value)
            cells.append(f"{value:.3f}" if isinstance(value, float) else str(value))
        rows.append(f"| **{src}** | " + " | ".join(cells) + " |")
    return "\n".join(rows)


def _format_markdown(report: dict[str, dict[str, dict]]) -> str:
    sections = [
        "# Binary Trees Cross-Language UAST Comparison",
        "",
        "Pairwise structural comparison of the binarytrees benchmark across "
        "`python`, `rust`, `javascript`, and `cpp`. All metrics operate on UAST "
        "`kind` values, so they are language-agnostic.",
        "",
        "## Kind-Histogram Distance (L1, [0, 1])",
        "",
        _format_matrix(report, "kind_distance"),
        "",
        "## UAST Edit Distance (normalized, [0, 1])",
        "",
        _format_matrix(report, "edit_distance"),
        "",
        "## Per-Pair Control-Flow Deltas",
        "",
    ]
    languages = list(report.keys())
    for i, src in enumerate(languages):
        for tgt in languages[i + 1 :]:
            entry = report[src][tgt]
            non_zero = {k: v for k, v in entry["control_flow_delta"].items() if v != 0}
            sections.append(f"### {src} -> {tgt}")
            sections.append("")
            sections.append(
                f"- Kind distance: `{entry['kind_distance']:.3f}`  "
                f"- Edit distance: `{entry['edit_distance']['normalized']:.3f}`  "
                f"- Detects difference: `{entry['detects_difference']}`"
            )
            if non_zero:
                sections.append(
                    "- Control-flow delta: "
                    + ", ".join(f"`{k}: {v:+d}`" for k, v in non_zero.items())
                )
            sections.append("")
    return "\n".join(sections)


def _print_console_summary(report: dict[str, dict[str, dict]]) -> None:
    print("Binary Trees UAST kind-distance matrix:")
    print(_format_matrix(report, "kind_distance"))
    print()
    print("Binary Trees UAST edit-distance matrix (normalized):")
    print(_format_matrix(report, "edit_distance"))


def main() -> None:
    src_dir = Path("demos/binarytrees/src")
    results_dir = Path("demos/binarytrees/results")
    results_dir.mkdir(parents=True, exist_ok=True)

    uasts = _load_uasts(src_dir)
    report = _pairwise_report(uasts)

    json_path = results_dir / "comparison.json"
    md_path = results_dir / "comparison.md"
    json_path.write_text(json.dumps(report, indent=2), encoding="utf-8")
    md_path.write_text(_format_markdown(report), encoding="utf-8")

    _print_console_summary(report)
    print()
    print(f"Wrote {json_path}")
    print(f"Wrote {md_path}")


if __name__ == "__main__":
    main()
