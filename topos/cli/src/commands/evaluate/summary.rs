//! Compact cumulative rendering for `topos evaluate`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use console::Style;
use serde_json::{json, Value};
use topos_engine::core::characteristic_morphism::{CharacteristicMorphism, ClassificationResult};
use topos_engine::core::omega::{EvaluationValue, Generator};
use topos_engine::evaluation::preferences::RANKING_LEN;

/// Pillar rows, in `Generator::ALL` order so the table cannot fall out of
/// step with the algebra.
const PILLARS: [&str; RANKING_LEN] = [
    Generator::Simple.as_str(),
    Generator::Composable.as_str(),
    Generator::Secure.as_str(),
    Generator::Navigable.as_str(),
];

use crate::commands::render::{
    guide as guide_char, guide_line, paint, truncate_left, RenderOptions,
};

struct SummaryView<'a> {
    language: &'a str,
    options: RenderOptions,
    composable_requested: bool,
    show_info_hint: bool,
    composable_notices: &'a [String],
    action: &'a str,
}

pub(crate) fn print_summary(
    files: &[PathBuf],
    results: &[ClassificationResult],
    language: &str,
    composable_requested: bool,
    show_info_hint: bool,
    composable_notices: &[String],
) {
    for line in render_summary(
        files,
        results,
        SummaryView {
            language,
            options: RenderOptions::stdout(),
            composable_requested,
            show_info_hint,
            composable_notices,
            action: "Evaluated",
        },
    ) {
        println!("{line}");
    }
}

pub(crate) fn print_inspection_summary(
    path: &PathBuf,
    result: &ClassificationResult,
    language: &str,
    composable_notices: &[String],
) {
    for line in render_summary(
        std::slice::from_ref(path),
        std::slice::from_ref(result),
        SummaryView {
            language,
            options: RenderOptions::stdout(),
            composable_requested: false,
            show_info_hint: false,
            composable_notices,
            action: "Inspected",
        },
    ) {
        println!("{line}");
    }
}

fn render_summary(
    files: &[PathBuf],
    results: &[ClassificationResult],
    view: SummaryView<'_>,
) -> Vec<String> {
    let SummaryView {
        language,
        options,
        composable_requested,
        show_info_hint,
        composable_notices,
        action,
    } = view;
    let mut lines = Vec::new();
    let single_file = results.len() == 1;
    let title = if single_file {
        files.first().map_or_else(
            || format!("◇  {action} 1 file"),
            |path| format!("◇  {action} {}", path.display()),
        )
    } else {
        format!("◇  {action} {} files", results.len())
    };
    lines.push(paint(title, Style::new().bold(), options));
    let composable = results
        .iter()
        .any(|result| result.scores.contains_key("composable"));
    if let Some(result) = results.first() {
        lines.push(guide_line(
            format!(
                "{language} · priority {} · COMPOSABLE {}",
                crate::commands::config::priority_name(result.priority),
                if composable {
                    "enabled"
                } else {
                    "not measured"
                }
            ),
            Style::new().dim(),
            options,
        ));
    }
    for notice in human_composable_notices(composable_notices) {
        lines.push(composable_notice_line(&notice, options));
    }
    lines.push(guide_char('│', options));

    let show_rail = options.width >= 80;
    let rail_width = if options.width >= 100 { 15 } else { 10 };
    let header = if single_file && show_rail {
        format!(
            "│  {:<12}  {:<6}  {:>5}   QUALITY",
            "PILLAR", "STATUS", "SCORE"
        )
    } else if single_file {
        format!("│  {:<12}  {:<6}  {:>5}", "PILLAR", "STATUS", "SCORE")
    } else if show_rail {
        format!(
            "│  {:<12}  {:<6}  {:>5}  {:>5}  {:>9}   SCORE",
            "PILLAR", "STATUS", "AVG", "MIN", "FAILURES"
        )
    } else {
        format!(
            "│  {:<12}  {:<6}  {:>5}  {:>5}  {:>9}",
            "PILLAR", "STATUS", "AVG", "MIN", "FAILURES"
        )
    };
    lines.push(guide_line(
        header.strip_prefix("│  ").unwrap_or(&header),
        Style::new().bold().dim(),
        options,
    ));

    let overall = CharacteristicMorphism.combine_dimensions(results);
    for pillar in PILLARS {
        let scores: Vec<f64> = results
            .iter()
            .filter_map(|result| result.scores.get(pillar).copied())
            .collect();
        if scores.is_empty() {
            continue;
        }
        let average = scores.iter().sum::<f64>() / scores.len() as f64;
        let minimum = scores.iter().copied().fold(f64::INFINITY, f64::min);
        let failures = failure_count(results, pillar);
        let passing = overall
            .get(pillar)
            .is_some_and(|value| *value != EvaluationValue::Slop);
        let status = status_text(passing, average, options);
        let mut row = if single_file {
            format!(
                "{}  {:<12}  {status}  {:>4.0}%",
                guide_char('│', options),
                pillar.to_ascii_uppercase(),
                average * 100.0,
            )
        } else {
            format!(
                "{}  {:<12}  {status}  {:>4.0}%  {:>4.0}%  {:>4} / {:<3}",
                guide_char('│', options),
                pillar.to_ascii_uppercase(),
                average * 100.0,
                minimum * 100.0,
                failures,
                results.len(),
            )
        };
        if show_rail {
            row.push_str(&format!("   {}", score_rail(average, rail_width)));
        }
        lines.push(row);
    }

    let mean = results.iter().map(mean_score).sum::<f64>() / results.len() as f64;
    let floor = floor_verdict(&overall);
    lines.push(guide_char('│', options));
    lines.push(guide_line(
        "Status reflects policy gates; scores are diagnostic — use them to guide refactoring.",
        Style::new().dim(),
        options,
    ));
    lines.push(floor_line(floor, mean, options));

    // COMPOSABLE recovery is part of the finished card whenever setup warnings
    // exist — not gated on --info. Mid-run stderr stays quiet.
    let mut tips = Vec::new();
    if composable_requested && wants_depgraph_recovery_tip(composable_notices) {
        tips.push(depgraph_recovery_tip(composable_notices));
    }
    if show_info_hint {
        if results.len() == 1 {
            tips.push("Tip: use `topos inspect` for metrics, functions, and guidance.".to_string());
        } else if let Some((pillar, count)) = hinted_failure(results) {
            tips.push(format!(
                "Tip: add --failures {pillar} to list its {count} failing file{}; --info shows overall weak spots.",
                if count == 1 { "" } else { "s" }
            ));
        } else {
            tips.push("Tip: add --info to inspect the five weakest files.".to_string());
        }
    }
    if !tips.is_empty() {
        lines.push(String::new());
        for tip in tips {
            lines.push(paint(tip, Style::new().dim(), options));
        }
    }
    lines
}

/// Human card notices derived from machine `warnings`.
///
/// JSON keeps the full list; the card collapses overlapping setup reasons so
/// "branch not indexed" does not also show a stale warning about another
/// branch's store, and mid-run stderr is not required for the same facts.
fn human_composable_notices(warnings: &[String]) -> Vec<String> {
    let hard_miss = warnings.iter().any(|warning| {
        matches!(
            notice_kind(warning),
            NoticeKind::BranchNotIndexed | NoticeKind::MissingStore | NoticeKind::LoadFailure
        )
    });

    let mut notices = Vec::new();
    for warning in warnings {
        let kind = notice_kind(warning);
        if hard_miss && kind == NoticeKind::Stale {
            continue;
        }
        let notice = humanize_composable_notice(warning);
        if notice.is_empty() {
            continue;
        }
        if notices.iter().any(|existing| existing == &notice) {
            continue;
        }
        notices.push(notice);
    }
    notices
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum NoticeKind {
    BranchNotIndexed,
    MissingStore,
    LoadFailure,
    Schema,
    Stale,
    Other,
}

fn notice_kind(warning: &str) -> NoticeKind {
    let lower = warning.to_ascii_lowercase();
    if lower.contains("no gitnexus store indexed for branch")
        || lower.contains("current branch is not indexed")
    {
        NoticeKind::BranchNotIndexed
    } else if lower.contains("no .gitnexus directory found") {
        NoticeKind::MissingStore
    } else if lower.contains("could not be loaded") || lower.contains("no indexed store found") {
        NoticeKind::LoadFailure
    } else if lower.contains("storage version") || lower.contains("ladybug") {
        NoticeKind::Schema
    } else if lower.contains("may be stale") || lower.contains("stale") {
        NoticeKind::Stale
    } else {
        NoticeKind::Other
    }
}

/// Short in-card copy for one COMPOSABLE setup warning (machine strings stay full in JSON).
fn humanize_composable_notice(warning: &str) -> String {
    let trimmed = warning
        .trim()
        .strip_prefix("COMPOSABLE not scored — ")
        .unwrap_or(warning.trim());
    let lower = trimmed.to_ascii_lowercase();

    if lower.contains("no gitnexus store indexed for branch")
        || lower.contains("current branch is not indexed")
    {
        return format_branch_not_indexed_notice(trimmed);
    }
    if lower.contains("no .gitnexus directory found") {
        return "no .gitnexus store yet".to_string();
    }
    if lower.contains("could not be loaded") || lower.contains("no indexed store found") {
        return "dependency graph could not be loaded".to_string();
    }
    if lower.contains("storage version") || lower.contains("ladybug") {
        return "GitNexus store schema mismatch".to_string();
    }
    if lower.contains("may be stale") || lower.contains("stale") {
        return "index may be stale".to_string();
    }
    // Keep invalid-override and other detail readable but strip trailing
    // "evaluating SIMPLE/SECURE/NAVIGABLE only" noise — the meta line already says so.
    trimmed
        .trim_end_matches('.')
        .trim_end_matches(" — evaluating SIMPLE/SECURE/NAVIGABLE only")
        .trim_end_matches("; evaluating SIMPLE/SECURE/NAVIGABLE only")
        // Pre-v0.5.0 phrasing, still emitted by some notice paths.
        .trim_end_matches(" — evaluating SIMPLE/SECURE only")
        .trim_end_matches("; evaluating SIMPLE/SECURE only")
        .to_string()
}

fn format_branch_not_indexed_notice(message: &str) -> String {
    // Machine shapes:
    //   no gitnexus store indexed for branch 'x' (indexed: main, foo)
    //   no gitnexus store indexed for branch x; indexed: main, foo
    //   current branch is not indexed (indexed: main)
    let lower = message.to_ascii_lowercase();
    let indexed = if let Some(idx) = lower.find("(indexed:") {
        message[idx + "(indexed:".len()..]
            .trim()
            .trim_end_matches(')')
            .trim()
    } else if let Some(idx) = lower.find("; indexed:") {
        message[idx + "; indexed:".len()..]
            .trim()
            .trim_end_matches('.')
            .trim()
    } else if let Some(idx) = lower.find("indexed:") {
        message[idx + "indexed:".len()..]
            .trim()
            .trim_start_matches(')')
            .trim_end_matches(')')
            .trim()
    } else {
        ""
    };
    if indexed.is_empty() {
        "branch not indexed".to_string()
    } else {
        format!("branch not indexed (have: {indexed})")
    }
}

fn composable_notice_line(notice: &str, options: RenderOptions) -> String {
    format!(
        "{}  {} {}",
        guide_char('│', options),
        paint("↻", Style::new().color256(208), options),
        paint(notice, Style::new().dim(), options),
    )
}

fn wants_depgraph_recovery_tip(notices: &[String]) -> bool {
    notices.iter().any(|warning| {
        matches!(
            notice_kind(warning),
            NoticeKind::BranchNotIndexed
                | NoticeKind::MissingStore
                | NoticeKind::LoadFailure
                | NoticeKind::Schema
                | NoticeKind::Stale
        ) || {
            let lower = warning.to_ascii_lowercase();
            lower.contains("depgraph generate") || lower.contains("topos depgraph")
        }
    })
}

fn depgraph_recovery_tip(notices: &[String]) -> String {
    if notices
        .iter()
        .any(|warning| notice_kind(warning) == NoticeKind::BranchNotIndexed)
    {
        "Tip: run `topos depgraph generate` to index this branch for COMPOSABLE.".to_string()
    } else if notices
        .iter()
        .any(|warning| notice_kind(warning) == NoticeKind::Stale)
    {
        "Tip: run `topos depgraph generate` to refresh COMPOSABLE.".to_string()
    } else {
        "Tip: run `topos depgraph generate` to enable COMPOSABLE scoring.".to_string()
    }
}

pub(crate) fn pillar_measured(results: &[ClassificationResult], pillar: &str) -> bool {
    results
        .iter()
        .any(|result| result.scores.contains_key(pillar))
}

pub(crate) fn pillar_failed(result: &ClassificationResult, pillar: &str) -> bool {
    !result.is_parseable || result.dimensions.get(pillar).copied() == Some(EvaluationValue::Slop)
}

fn failure_count(results: &[ClassificationResult], pillar: &str) -> usize {
    results
        .iter()
        .filter(|result| pillar_failed(result, pillar))
        .count()
}

fn hinted_failure(results: &[ClassificationResult]) -> Option<(&'static str, usize)> {
    let priority = results
        .first()
        .map(|result| crate::commands::config::priority_name(result.priority));
    priority.into_iter().chain(PILLARS).find_map(|pillar| {
        let count = failure_count(results, pillar);
        (pillar_measured(results, pillar) && count > 0).then_some((pillar, count))
    })
}

pub(crate) fn attention_lines(
    files: &[PathBuf],
    results: &[ClassificationResult],
    selected: Option<usize>,
    styled: bool,
    width: usize,
) -> Vec<String> {
    attention_lines_with_options(files, results, selected, RenderOptions { styled, width })
}

pub(crate) fn failure_lines(
    files: &[PathBuf],
    results: &[ClassificationResult],
    indices: &[usize],
    pillar: &str,
    selected: Option<usize>,
    styled: bool,
    width: usize,
) -> Vec<String> {
    let options = RenderOptions { styled, width };
    let display_root = common_parent(files);
    let path_width = width.saturating_sub(20).clamp(20, 60);
    let total = failure_count(results, pillar);
    let mut lines = vec![
        String::new(),
        paint(
            format!(
                "{} failures · {} file{}",
                pillar.to_ascii_uppercase(),
                total,
                if total == 1 { "" } else { "s" }
            ),
            Style::new().cyan().bold(),
            options,
        ),
    ];
    if indices.len() < total {
        lines.push(paint(
            format!("showing {} lowest pillar scores", indices.len()),
            Style::new().dim(),
            options,
        ));
    }
    if selected.is_some() {
        lines.push(paint(
            "↑↓ move · enter open · esc close",
            Style::new().dim(),
            options,
        ));
    }
    lines.push(String::new());
    if indices.is_empty() {
        lines.push(paint(
            format!("No files fail {}.", pillar.to_ascii_uppercase()),
            Style::new().dim(),
            options,
        ));
        return lines;
    }
    for (rank, result_index) in indices.iter().copied().enumerate() {
        let path = &files[result_index];
        let result = &results[result_index];
        let display_path = display_root
            .as_deref()
            .and_then(|root| path.strip_prefix(root).ok())
            .unwrap_or(path)
            .display()
            .to_string();
        let marker = if selected == Some(rank) { '›' } else { ' ' };
        let detail = if result.is_parseable {
            format!(
                "{:>3.0}% score",
                result.scores.get(pillar).copied().unwrap_or(0.0) * 100.0
            )
        } else {
            "parse failure".to_string()
        };
        lines.push(format!(
            "{marker} {:>2}  {:<path_width$}  {detail}",
            rank + 1,
            truncate_left(&display_path, path_width),
        ));
    }
    lines
}

fn attention_lines_with_options(
    files: &[PathBuf],
    results: &[ClassificationResult],
    selected: Option<usize>,
    options: RenderOptions,
) -> Vec<String> {
    let ranked = ranked_file_indices(files, results, 5);
    let display_root = common_parent(files);
    let path_width = options.width.saturating_sub(32).clamp(20, 52);
    let mut lines = vec![
        String::new(),
        paint("Weak spots", Style::new().cyan().bold(), options),
    ];
    if selected.is_some() {
        lines.push(paint(
            "↑↓ move · enter open · esc close",
            Style::new().dim(),
            options,
        ));
    }
    lines.push(String::new());
    for (rank, result_index) in ranked.into_iter().enumerate() {
        let path = &files[result_index];
        let result = &results[result_index];
        let display_path = display_root
            .as_deref()
            .and_then(|root| path.strip_prefix(root).ok())
            .unwrap_or(path)
            .display()
            .to_string();
        let verdict = verdict_text(result.summary(), options);
        let marker = if selected == Some(rank) { '›' } else { ' ' };
        lines.push(format!(
            "{marker} {:>2}  {:<path_width$}  {:>3.0}% avg · {verdict}",
            rank + 1,
            truncate_left(&display_path, path_width),
            mean_score(result) * 100.0,
        ));
    }
    lines
}

pub(crate) fn json_output(
    files: &[PathBuf],
    results: &[ClassificationResult],
    languages: &[String],
    warnings: &[String],
) -> Value {
    let rows: Vec<Value> = files
        .iter()
        .zip(results)
        .enumerate()
        .map(|(index, (path, result))| {
            let dimensions: serde_json::Map<String, Value> = result
                .dimensions
                .iter()
                .map(|(name, value)| (name.clone(), json!(value.name())))
                .collect();
            let scores: serde_json::Map<String, Value> = result
                .scores
                .iter()
                .map(|(name, score)| (name.clone(), json!((score * 1000.0).round() / 10.0)))
                .collect();
            let language = languages
                .get(index)
                .map(String::as_str)
                .unwrap_or("unknown");
            json!({
                "file": path,
                "language": language,
                "is_parseable": result.is_parseable,
                "lattice_element": result.summary().name(),
                "lattice_symbol": result.summary().symbol(),
                "dimensions": dimensions,
                "scores": scores,
                "priority": crate::commands::config::priority_name(result.priority),
                "raw_metrics": result.raw_metrics,
            })
        })
        .collect();
    // Top-level `warnings` mirrors MCP evaluate payloads so machines can see
    // COMPOSABLE setup failures (invalid override, generation miss, …)
    // instead of only inferring them from a missing `composable` score.
    json!({
        "version": env!("CARGO_PKG_VERSION"),
        "results": rows,
        "warnings": warnings,
    })
}

pub(crate) fn ranked_file_indices(
    files: &[PathBuf],
    results: &[ClassificationResult],
    limit: usize,
) -> Vec<usize> {
    let mut indices: Vec<usize> = (0..files.len().min(results.len())).collect();
    indices.sort_by(|a, b| {
        mean_score(&results[*a])
            .total_cmp(&mean_score(&results[*b]))
            .then_with(|| files[*a].cmp(&files[*b]))
    });
    indices.truncate(limit);
    indices
}

pub(crate) fn failure_file_indices(
    files: &[PathBuf],
    results: &[ClassificationResult],
    pillar: &str,
) -> Vec<usize> {
    let mut indices: Vec<usize> = (0..files.len().min(results.len()))
        .filter(|index| pillar_failed(&results[*index], pillar))
        .collect();
    indices.sort_by(|a, b| {
        results[*a]
            .scores
            .get(pillar)
            .copied()
            .unwrap_or(0.0)
            .total_cmp(&results[*b].scores.get(pillar).copied().unwrap_or(0.0))
            .then_with(|| files[*a].cmp(&files[*b]))
    });
    indices
}

fn status_text(passing: bool, score: f64, options: RenderOptions) -> String {
    let (symbol, label, style) = if !passing {
        ("X", "FAIL", Style::new().red().bold())
    } else if score < 0.25 {
        ("!", "WARN", Style::new().yellow().bold())
    } else {
        ("✓", "PASS", Style::new().green().bold())
    };
    format!("{} {label}", paint(symbol, style, options))
}

/// Medal banding lives in `Ω`, not here — see
/// [`EvaluationValue::medal_tier`].
fn medal_tier_name(floor: EvaluationValue) -> &'static str {
    floor.medal_tier()
}

fn floor_line(floor: EvaluationValue, mean: f64, options: RenderOptions) -> String {
    match floor {
        // SLOP is already the failure tier — no medal emoji and no second
        // lattice-name echo (`X ❌ SLOP · SLOP` read as four failure cues).
        EvaluationValue::Slop => format!(
            "{}  {}  {} · {:.0}% average.",
            guide_char('└', options),
            paint("X", Style::new().red().bold(), options),
            paint(medal_tier_name(floor), Style::new().red().bold(), options),
            mean * 100.0,
        ),
        other => {
            let style = Style::new().green().bold();
            format!(
                "{}  {} {} {} · {} · {:.0}% average.",
                guide_char('└', options),
                paint("✓", style.clone(), options),
                other.symbol(),
                medal_tier_name(other),
                paint(other.name(), style, options),
                mean * 100.0,
            )
        }
    }
}

fn verdict_text(verdict: EvaluationValue, options: RenderOptions) -> String {
    let style = match verdict {
        EvaluationValue::Slop => Style::new().red(),
        _ => Style::new().green(),
    };
    paint(verdict.name(), style, options)
}

fn mean_score(result: &ClassificationResult) -> f64 {
    if result.scores.is_empty() {
        return 0.0;
    }
    result.scores.values().sum::<f64>() / result.scores.len() as f64
}

fn score_rail(score: f64, width: usize) -> String {
    let marker = (score.clamp(0.0, 1.0) * (width.saturating_sub(1)) as f64).round() as usize;
    (0..width)
        .map(|index| {
            if index == marker {
                '◆'
            } else if index < marker {
                '━'
            } else {
                '─'
            }
        })
        .collect()
}

fn floor_verdict(overall: &BTreeMap<String, EvaluationValue>) -> EvaluationValue {
    let bits = PILLARS.iter().fold(0, |bits, pillar| {
        bits | overall
            .get(*pillar)
            .filter(|value| **value != EvaluationValue::Slop)
            .map_or(0, |value| value.bits())
    });
    EvaluationValue::from_bits(bits).unwrap_or(EvaluationValue::Slop)
}

fn common_parent(files: &[PathBuf]) -> Option<PathBuf> {
    let mut common = files.first()?.parent()?.to_path_buf();
    for file in &files[1..] {
        let parent = file.parent()?;
        while !parent.starts_with(&common) {
            if !common.pop() {
                return None;
            }
        }
    }
    (!common.as_os_str().is_empty()).then_some(common)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use topos_engine::evaluation::policies::base::Priority;

    use super::*;

    fn result(simple_passes: bool) -> ClassificationResult {
        ClassificationResult {
            is_parseable: true,
            dimensions: BTreeMap::from([(
                "simple".to_string(),
                if simple_passes {
                    EvaluationValue::Simple
                } else {
                    EvaluationValue::Slop
                },
            )]),
            scores: BTreeMap::from([("simple".to_string(), 0.6)]),
            lattice_element: if simple_passes {
                EvaluationValue::Simple
            } else {
                EvaluationValue::Slop
            },
            ..Default::default()
        }
    }

    #[test]
    fn score_rail_has_one_marker_and_no_blocks() {
        assert_eq!(score_rail(0.6, 10), "━━━━━◆────");
        assert_eq!(score_rail(2.0, 4), "━━━◆");
    }

    #[test]
    fn plain_summary_points_to_exact_failures_and_overall_info() {
        let files = vec![
            PathBuf::from("topos/a.rs"),
            PathBuf::from("topos/nested/b.rs"),
        ];
        let output = render_summary(
            &files,
            &[result(false), result(true)],
            SummaryView {
                language: "rust",
                options: RenderOptions {
                    styled: false,
                    width: 120,
                },
                composable_requested: false,
                show_info_hint: true,
                composable_notices: &[],
                action: "Evaluated",
            },
        )
        .join("\n");

        assert!(output.contains("X FAIL"));
        assert!(output.contains(
            "Tip: add --failures simple to list its 1 failing file; --info shows overall weak spots."
        ));
        assert!(!output.contains("Weak spots"));
        assert!(output.contains("X  SLOP ·"));
        assert!(!output.contains("SLOP · SLOP"));
        assert!(!output.contains('❌'));
        assert!(!output.contains('🥉'));
    }

    #[test]
    fn summary_hint_prefers_a_failing_priority_pillar() {
        let mut first = result(false);
        first.priority = Priority::Secure;
        first
            .dimensions
            .insert("secure".to_string(), EvaluationValue::Slop);
        first.scores.insert("secure".to_string(), 0.8);
        let mut second = result(true);
        second.priority = Priority::Secure;
        second
            .dimensions
            .insert("secure".to_string(), EvaluationValue::Secure);
        second.scores.insert("secure".to_string(), 0.2);
        let output = render_summary(
            &[PathBuf::from("a.rs"), PathBuf::from("b.rs")],
            &[first, second],
            SummaryView {
                language: "rust",
                options: RenderOptions {
                    styled: false,
                    width: 120,
                },
                composable_requested: false,
                show_info_hint: true,
                composable_notices: &[],
                action: "Evaluated",
            },
        )
        .join("\n");

        assert!(output.contains("--failures secure to list its 1 failing file"));
    }

    #[test]
    fn all_pillars_passing_keeps_the_general_info_hint() {
        let output = render_summary(
            &[PathBuf::from("a.rs"), PathBuf::from("b.rs")],
            &[result(true), result(true)],
            SummaryView {
                language: "rust",
                options: RenderOptions {
                    styled: false,
                    width: 120,
                },
                composable_requested: false,
                show_info_hint: true,
                composable_notices: &[],
                action: "Evaluated",
            },
        )
        .join("\n");

        assert!(output.contains("Tip: add --info to inspect the five weakest files."));
    }

    #[test]
    fn requested_unmeasured_composable_without_notice_keeps_inspect_tip() {
        let output = render_summary(
            &[PathBuf::from("a.rs")],
            &[result(true)],
            SummaryView {
                language: "rust",
                options: RenderOptions {
                    styled: false,
                    width: 120,
                },
                composable_requested: true,
                show_info_hint: true,
                composable_notices: &[],
                action: "Evaluated",
            },
        )
        .join("\n");

        assert!(!output.contains("topos depgraph generate"));
        assert!(output.contains("Tip: use `topos inspect` for metrics, functions, and guidance."));
    }

    #[test]
    fn branch_not_indexed_notice_and_recovery_tip() {
        let notice = "COMPOSABLE not scored — no gitnexus store indexed for branch 'feat/x' (indexed: main, release/v0.4.4)";
        let output = render_summary(
            &[PathBuf::from("a.rs"), PathBuf::from("b.rs")],
            &[result(false), result(true)],
            SummaryView {
                language: "rust",
                options: RenderOptions {
                    styled: false,
                    width: 120,
                },
                composable_requested: true,
                show_info_hint: true,
                composable_notices: &[notice.to_string()],
                action: "Evaluated",
            },
        )
        .join("\n");

        assert!(
            output.contains("↻ branch not indexed (have: main, release/v0.4.4)"),
            "{output}"
        );
        assert!(!output.contains("gitnexus:"));
        assert!(output
            .contains("Tip: run `topos depgraph generate` to index this branch for COMPOSABLE."));
        assert!(output.contains("Tip: add --failures simple"));
        assert!(output.contains("X  SLOP ·"));
        assert!(!output.contains('❌'));
    }

    #[test]
    fn human_card_drops_stale_when_branch_is_not_indexed() {
        let notices = [
            "gitnexus index may be stale — source tree content changed since the dependency graph was generated; run 'topos depgraph generate' before trusting COMPOSABLE.".to_string(),
            "COMPOSABLE not scored — no gitnexus store indexed for branch 'release/044-issue-fixes'; indexed: main, codex/restore-cli-terminal-ux".to_string(),
        ];
        let human = human_composable_notices(&notices);
        assert_eq!(
            human,
            vec!["branch not indexed (have: main, codex/restore-cli-terminal-ux)".to_string()]
        );

        let output = render_summary(
            &[PathBuf::from("a.rs")],
            &[result(false)],
            SummaryView {
                language: "rust",
                options: RenderOptions {
                    styled: false,
                    width: 120,
                },
                composable_requested: true,
                show_info_hint: false,
                composable_notices: &notices,
                action: "Evaluated",
            },
        )
        .join("\n");

        assert!(
            output.contains("↻ branch not indexed (have: main, codex/restore-cli-terminal-ux)"),
            "{output}"
        );
        assert!(!output.contains("stale"), "{output}");
        assert!(
            output.contains(
                "Tip: run `topos depgraph generate` to index this branch for COMPOSABLE."
            ),
            "{output}"
        );
        // JSON still receives the full machine list from the caller — only the
        // human card collapses overlapping setup reasons.
        assert_eq!(notices.len(), 2);
    }

    #[test]
    fn stale_only_notice_uses_refresh_tip() {
        let notices = [
            "gitnexus index may be stale — source tree content changed since the dependency graph was generated; run 'topos depgraph generate' before trusting COMPOSABLE.".to_string(),
        ];
        assert_eq!(
            human_composable_notices(&notices),
            vec!["index may be stale".to_string()]
        );

        let mut result = result(true);
        result
            .dimensions
            .insert("composable".to_string(), EvaluationValue::Composable);
        result.scores.insert("composable".to_string(), 0.9);

        let output = render_summary(
            &[PathBuf::from("a.rs")],
            &[result],
            SummaryView {
                language: "rust",
                options: RenderOptions {
                    styled: false,
                    width: 120,
                },
                composable_requested: true,
                show_info_hint: false,
                composable_notices: &notices,
                action: "Evaluated",
            },
        )
        .join("\n");

        assert!(output.contains("↻ index may be stale"), "{output}");
        assert!(
            output.contains("Tip: run `topos depgraph generate` to refresh COMPOSABLE."),
            "{output}"
        );
    }

    #[test]
    fn floor_line_shows_platinum_medal_and_lattice_name() {
        let line = floor_line(
            EvaluationValue::Ideal,
            0.67,
            RenderOptions {
                styled: false,
                width: 120,
            },
        );
        assert!(line.contains("✓"));
        assert!(line.contains("PLATINUM · IDEAL · 67% average."));
    }

    /// All four pillars is PLATINUM; three of four is GOLD.
    #[test]
    fn floor_line_shows_gold_medal_for_three_of_four_pillars() {
        let line = floor_line(
            EvaluationValue::SimpleComposableSecure,
            0.67,
            RenderOptions {
                styled: false,
                width: 120,
            },
        );
        assert!(line.contains("GOLD · SIMPLE_COMPOSABLE_SECURE · 67% average."));
    }

    #[test]
    fn floor_line_shows_silver_medal_and_lattice_name() {
        let line = floor_line(
            EvaluationValue::SimpleSecure,
            0.8,
            RenderOptions {
                styled: false,
                width: 120,
            },
        );
        assert!(line.contains("✓"));
        assert!(line.contains("SILVER · SIMPLE_SECURE · 80% average."));
    }

    #[test]
    fn floor_line_shows_slop_without_pass_glyph() {
        let line = floor_line(
            EvaluationValue::Slop,
            0.2,
            RenderOptions {
                styled: false,
                width: 120,
            },
        );
        assert!(line.contains("X"));
        assert!(line.contains("X  SLOP · 20% average."));
        assert!(!line.contains('❌'));
        assert!(!line.contains("SLOP · SLOP"));
    }

    #[test]
    fn intentionally_skipped_composable_has_no_recovery_hint() {
        let output = render_summary(
            &[PathBuf::from("a.rs")],
            &[result(true)],
            SummaryView {
                language: "rust",
                options: RenderOptions {
                    styled: false,
                    width: 120,
                },
                composable_requested: false,
                show_info_hint: false,
                composable_notices: &[],
                action: "Evaluated",
            },
        )
        .join("\n");

        assert!(!output.contains("topos depgraph generate"));
    }

    #[test]
    fn styled_summary_colors_failure_and_pass_symbols() {
        let failure = status_text(
            false,
            0.5,
            RenderOptions {
                styled: true,
                width: 120,
            },
        );
        let pass = status_text(
            true,
            0.5,
            RenderOptions {
                styled: true,
                width: 120,
            },
        );
        let warn = status_text(
            true,
            0.2,
            RenderOptions {
                styled: true,
                width: 120,
            },
        );
        assert!(failure.contains("\u{1b}[31m"));
        assert!(pass.contains("\u{1b}[32m"));
        assert!(warn.contains("\u{1b}[33m"));
        assert!(warn.contains("WARN"));
    }

    #[test]
    fn low_score_passing_renders_warn_status() {
        let mut low = result(true);
        low.scores.insert("simple".to_string(), 0.1);
        let output = render_summary(
            &[PathBuf::from("low.rs")],
            &[low],
            SummaryView {
                language: "rust",
                options: RenderOptions {
                    styled: false,
                    width: 120,
                },
                composable_requested: false,
                show_info_hint: false,
                composable_notices: &[],
                action: "Evaluated",
            },
        )
        .join("\n");
        assert!(output.contains("! WARN"));
        assert!(!output.contains("✓ PASS"));
    }

    #[test]
    fn styled_partial_verdict_names_passed_gates_in_green() {
        let verdict = verdict_text(
            EvaluationValue::SimpleSecure,
            RenderOptions {
                styled: true,
                width: 120,
            },
        );
        assert!(verdict.contains("\u{1b}[32m"));
        assert!(verdict.contains("SIMPLE_SECURE"));
    }

    #[test]
    fn dim_content_keeps_the_guide_white() {
        let line = guide_line(
            "muted",
            Style::new().dim(),
            RenderOptions {
                styled: true,
                width: 120,
            },
        );
        assert!(line.starts_with("\u{1b}[37m│\u{1b}[0m"));
        assert!(line.contains("\u{1b}[2m"));
    }

    #[test]
    fn weak_spots_rank_by_displayed_average() {
        let files = vec![PathBuf::from("balanced.rs"), PathBuf::from("weak.rs")];
        let mut balanced = result(true);
        balanced.scores =
            BTreeMap::from([("simple".to_string(), 0.5), ("secure".to_string(), 0.5)]);
        let mut weak = result(true);
        weak.scores = BTreeMap::from([("simple".to_string(), 0.2), ("secure".to_string(), 0.1)]);
        assert_eq!(
            ranked_file_indices(&files, &[balanced, weak], 5),
            vec![1, 0]
        );
    }

    #[test]
    fn weak_spots_show_average_before_the_verdict() {
        let mut scored = result(true);
        scored.scores = BTreeMap::from([
            ("simple".to_string(), 0.5),
            ("composable".to_string(), 0.0),
            ("secure".to_string(), 1.0),
        ]);
        scored.lattice_element = EvaluationValue::SimpleSecure;
        let output =
            attention_lines(&[PathBuf::from("a.rs")], &[scored], None, false, 100).join("\n");
        assert!(output.contains("50% avg · SIMPLE_SECURE"));
    }

    #[test]
    fn pillar_failures_use_gate_status_not_score() {
        let files = vec![PathBuf::from("passes.rs"), PathBuf::from("fails.rs")];
        let mut low_but_passing = result(true);
        low_but_passing.scores.insert("simple".to_string(), 0.1);
        let mut high_but_failing = result(false);
        high_but_failing.scores.insert("simple".to_string(), 0.9);

        assert_eq!(
            failure_file_indices(&files, &[low_but_passing, high_but_failing], "simple"),
            vec![1]
        );
    }

    #[test]
    fn pillar_failures_sort_by_that_pillar_and_label_parse_failures() {
        let files = vec![
            PathBuf::from("mid.rs"),
            PathBuf::from("low.rs"),
            PathBuf::from("parse.rs"),
        ];
        let mut mid = result(false);
        mid.scores.insert("simple".to_string(), 0.5);
        let mut low = result(false);
        low.scores.insert("simple".to_string(), 0.2);
        let parse = ClassificationResult::default();
        let results = vec![mid, low, parse];
        let ranked = failure_file_indices(&files, &results, "simple");

        assert_eq!(ranked, vec![2, 1, 0]);
        let output =
            failure_lines(&files, &results, &ranked, "simple", None, false, 100).join("\n");
        assert!(output.contains("SIMPLE failures · 3 files"));
        assert!(output.contains("parse.rs"));
        assert!(output.contains("parse failure"));
    }

    #[test]
    fn bounded_failure_browser_keeps_the_total_visible() {
        let files: Vec<PathBuf> = (0..6)
            .map(|index| PathBuf::from(format!("{index}.rs")))
            .collect();
        let results = vec![result(false); 6];
        let output = failure_lines(
            &files,
            &results,
            &[0, 1, 2, 3, 4],
            "simple",
            Some(0),
            false,
            100,
        )
        .join("\n");

        assert!(output.contains("SIMPLE failures · 6 files"));
        assert!(output.contains("showing 5 lowest pillar scores"));
    }

    #[test]
    fn narrow_summary_omits_score_rails() {
        let output = render_summary(
            &[PathBuf::from("a.rs")],
            &[result(true)],
            SummaryView {
                language: "rust",
                options: RenderOptions {
                    styled: false,
                    width: 72,
                },
                composable_requested: false,
                show_info_hint: true,
                composable_notices: &[],
                action: "Evaluated",
            },
        )
        .join("\n");
        assert!(!output.contains("QUALITY"));
        assert!(!output.contains('◆'));
    }

    #[test]
    fn single_file_summary_keeps_identity_without_aggregate_columns() {
        let output = render_summary(
            &[PathBuf::from("src/main.rs")],
            &[result(true)],
            SummaryView {
                language: "rust",
                options: RenderOptions {
                    styled: false,
                    width: 120,
                },
                composable_requested: false,
                show_info_hint: true,
                composable_notices: &[],
                action: "Evaluated",
            },
        )
        .join("\n");
        assert!(output.contains("◇  Evaluated src/main.rs"));
        assert!(output.contains("PILLAR        STATUS  SCORE   QUALITY"));
        assert!(!output.contains("AVG"));
        assert!(!output.contains("MIN"));
        assert!(!output.contains("FAILURES"));
        assert!(output.contains("Tip: use `topos inspect` for metrics, functions, and guidance."));
    }

    #[test]
    fn long_paths_are_bounded() {
        assert_eq!(
            truncate_left("abcdefghijklmnopqrstuvwxyz", 10),
            "…rstuvwxyz"
        );
    }
}
