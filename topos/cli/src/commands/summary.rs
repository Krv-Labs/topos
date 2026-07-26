//! Compact cumulative rendering for `topos evaluate`.

use std::path::PathBuf;

use console::Style;
use serde_json::{json, Value};
use topos_engine::core::characteristic_morphism::{CharacteristicMorphism, ClassificationResult};
use topos_engine::core::omega::EvaluationValue;

const PILLARS: [&str; 3] = ["simple", "composable", "secure"];

use super::render::{guide as guide_char, guide_line, paint, RenderOptions};

pub(crate) fn print_summary(
    files: &[PathBuf],
    results: &[ClassificationResult],
    language: &str,
    show_info_hint: bool,
) {
    let options = RenderOptions::stdout();
    for line in render_summary(
        files,
        results,
        language,
        options,
        show_info_hint,
        "Evaluated",
    ) {
        println!("{line}");
    }
}

pub(crate) fn print_inspection_summary(
    path: &PathBuf,
    result: &ClassificationResult,
    language: &str,
) {
    let options = RenderOptions::stdout();
    for line in render_summary(
        std::slice::from_ref(path),
        std::slice::from_ref(result),
        language,
        options,
        false,
        "Inspected",
    ) {
        println!("{line}");
    }
}

fn render_summary(
    files: &[PathBuf],
    results: &[ClassificationResult],
    language: &str,
    options: RenderOptions,
    show_info_hint: bool,
    action: &str,
) -> Vec<String> {
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
    if let Some(result) = results.first() {
        let composable = results
            .iter()
            .any(|result| result.scores.contains_key("composable"));
        lines.push(guide_line(
            format!(
                "{language} · priority {} · COMPOSABLE {}",
                super::config::priority_name(result.priority),
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
        let failures = results
            .iter()
            .filter(|result| {
                result
                    .dimensions
                    .get(pillar)
                    .is_none_or(|value| *value == EvaluationValue::Slop)
            })
            .count();
        let passing = overall
            .get(pillar)
            .is_some_and(|value| *value != EvaluationValue::Slop);
        let status = status_text(passing, options);
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
        "Status reflects policy gates; scores are diagnostic.",
        Style::new().dim(),
        options,
    ));
    lines.push(floor_line(floor, mean, options));

    if show_info_hint {
        lines.push(String::new());
        lines.push(paint(
            if results.len() == 1 {
                "Tip: use topos inspect for metrics, functions, and guidance."
            } else {
                "Tip: add --info to inspect the five weakest files."
            },
            Style::new().dim(),
            options,
        ));
    }
    lines
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

pub(crate) fn json_output(files: &[PathBuf], results: &[ClassificationResult]) -> Value {
    let rows: Vec<Value> = files
        .iter()
        .zip(results)
        .map(|(path, result)| {
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
            json!({
                "file": path,
                "is_parseable": result.is_parseable,
                "lattice_element": result.summary().name(),
                "lattice_symbol": result.summary().symbol(),
                "dimensions": dimensions,
                "scores": scores,
                "priority": super::config::priority_name(result.priority),
                "raw_metrics": result.raw_metrics,
            })
        })
        .collect();
    json!({ "version": env!("CARGO_PKG_VERSION"), "results": rows })
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

fn status_text(passing: bool, options: RenderOptions) -> String {
    let (symbol, label, style) = if passing {
        ("✓", "PASS", Style::new().green().bold())
    } else {
        ("X", "FAIL", Style::new().red().bold())
    };
    format!("{} {label}", paint(symbol, style, options))
}

fn floor_line(floor: EvaluationValue, mean: f64, options: RenderOptions) -> String {
    let (symbol, style) = match floor {
        EvaluationValue::Ideal => ("✓", Style::new().green().bold()),
        EvaluationValue::Slop => ("X", Style::new().red().bold()),
        _ => ("~", Style::new().yellow().bold()),
    };
    format!(
        "{}  {} {} floor · {:.0}% average",
        guide_char('└', options),
        paint(symbol, style.clone(), options),
        paint(floor.name(), style, options),
        mean * 100.0
    )
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

fn floor_verdict(overall: &std::collections::HashMap<String, EvaluationValue>) -> EvaluationValue {
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

fn truncate_left(value: &str, width: usize) -> String {
    let count = value.chars().count();
    if count <= width {
        return value.to_string();
    }
    format!(
        "…{}",
        value.chars().skip(count - width + 1).collect::<String>()
    )
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn result(simple_passes: bool) -> ClassificationResult {
        ClassificationResult {
            is_parseable: true,
            dimensions: HashMap::from([(
                "simple".to_string(),
                if simple_passes {
                    EvaluationValue::Simple
                } else {
                    EvaluationValue::Slop
                },
            )]),
            scores: HashMap::from([("simple".to_string(), 0.6)]),
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
    fn plain_summary_uses_text_symbols_and_points_to_info() {
        let files = vec![
            PathBuf::from("topos/a.rs"),
            PathBuf::from("topos/nested/b.rs"),
        ];
        let output = render_summary(
            &files,
            &[result(false), result(true)],
            "rust",
            RenderOptions {
                styled: false,
                width: 120,
            },
            true,
            "Evaluated",
        )
        .join("\n");

        assert!(output.contains("X FAIL"));
        assert!(output.contains("Tip: add --info to inspect the five weakest files."));
        assert!(!output.contains("Weak spots"));
        assert!(!output.contains('❌'));
        assert!(!output.contains('🥉'));
    }

    #[test]
    fn styled_summary_colors_failure_and_pass_symbols() {
        let failure = status_text(
            false,
            RenderOptions {
                styled: true,
                width: 120,
            },
        );
        let pass = status_text(
            true,
            RenderOptions {
                styled: true,
                width: 120,
            },
        );
        assert!(failure.contains("\u{1b}[31m"));
        assert!(pass.contains("\u{1b}[32m"));
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
        balanced.scores = HashMap::from([("simple".to_string(), 0.5), ("secure".to_string(), 0.5)]);
        let mut weak = result(true);
        weak.scores = HashMap::from([("simple".to_string(), 0.2), ("secure".to_string(), 0.1)]);
        assert_eq!(
            ranked_file_indices(&files, &[balanced, weak], 5),
            vec![1, 0]
        );
    }

    #[test]
    fn weak_spots_show_average_before_the_verdict() {
        let mut scored = result(true);
        scored.scores = HashMap::from([
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
    fn narrow_summary_omits_score_rails() {
        let output = render_summary(
            &[PathBuf::from("a.rs")],
            &[result(true)],
            "rust",
            RenderOptions {
                styled: false,
                width: 72,
            },
            true,
            "Evaluated",
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
            "rust",
            RenderOptions {
                styled: false,
                width: 120,
            },
            true,
            "Evaluated",
        )
        .join("\n");
        assert!(output.contains("◇  Evaluated src/main.rs"));
        assert!(output.contains("PILLAR        STATUS  SCORE   QUALITY"));
        assert!(!output.contains("AVG"));
        assert!(!output.contains("MIN"));
        assert!(!output.contains("FAILURES"));
        assert!(output.contains("Tip: use topos inspect for metrics, functions, and guidance."));
    }

    #[test]
    fn long_paths_are_bounded() {
        assert_eq!(
            truncate_left("abcdefghijklmnopqrstuvwxyz", 10),
            "…rstuvwxyz"
        );
    }
}
