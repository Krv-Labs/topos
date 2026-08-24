//! Pure terminal rendering for `topos evaluate --info` details.

use std::path::Path;

use console::Style;
use topos_engine::core::characteristic_morphism::ClassificationResult;
use topos_engine::core::omega::Generator;
use topos_engine::evaluation::suggestions::Suggestion;
use topos_mcp::schemas::RefactorTarget;

use crate::commands::render::{paint, truncate_right, RenderOptions};

pub(crate) struct FileDetails {
    pub(crate) targets: Vec<RefactorTarget>,
    pub(crate) suggestions: Vec<Suggestion>,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn detail_lines(
    path: &Path,
    result: &ClassificationResult,
    details: &FileDetails,
    width: usize,
    styled: bool,
    interactive: bool,
    rank: usize,
    total: usize,
) -> Vec<String> {
    let mut lines = Vec::new();
    let (weakest_pillar, weakest) = weakest_dimension(result);
    lines.push(paint(
        format!("◇  {}", path.display()),
        Style::new().bold(),
        RenderOptions { styled, width },
    ));
    lines.push(format!(
        "│  {} · weakest {} {:.0}%",
        result.summary().name(),
        weakest_pillar.to_ascii_uppercase(),
        weakest * 100.0
    ));
    if interactive {
        lines.push("│  esc back · q close".to_string());
    }
    if details.targets.is_empty() {
        lines.push("│".to_string());
        push_wrapped(
            &mut lines,
            "│  ",
            "No concrete target is available; run `topos inspect` for raw metrics.",
            width,
        );
        lines.push(format!("└  file {rank} of {total}"));
        return lines;
    }
    lines.push("│".to_string());
    lines.extend(recommendation_lines(details, width, styled, true));
    lines.push(format!(
        "└  file {rank} of {total} · {} ranked change{}",
        details.targets.len(),
        if details.targets.len() == 1 { "" } else { "s" }
    ));
    lines
}

pub(crate) fn recommendation_lines(
    details: &FileDetails,
    width: usize,
    styled: bool,
    guided: bool,
) -> Vec<String> {
    if details.targets.is_empty() {
        return Vec::new();
    }
    let prefix = if guided { "│  " } else { "  " };
    let item_prefix = if guided { "│  " } else { "  " };
    let mut lines = vec![format!(
        "{prefix}{}",
        paint(
            "Recommended changes",
            Style::new().bold(),
            RenderOptions { styled, width }
        )
    )];
    for (index, target) in details.targets.iter().enumerate() {
        lines.push(if guided { "│" } else { "" }.to_string());
        push_target_lines(
            &mut lines,
            index + 1,
            target,
            &details.suggestions,
            width,
            styled,
            item_prefix,
        );
    }
    lines
}

fn push_target_lines(
    lines: &mut Vec<String>,
    index: usize,
    target: &RefactorTarget,
    suggestions: &[Suggestion],
    width: usize,
    styled: bool,
    prefix: &str,
) {
    let (marker, marker_style) = if target.severity == "fix" {
        ("X", Style::new().red().bold())
    } else {
        ("~", Style::new().yellow().bold())
    };
    let pillar = target
        .failing_generators
        .first()
        .map_or("QUALITY", |value| value.as_str())
        .to_ascii_uppercase();
    lines.push(format!(
        "{prefix}{index}. {} {} · {pillar}",
        paint(marker, marker_style, RenderOptions { styled, width }),
        target.severity.to_ascii_uppercase(),
    ));
    let guide = prefix.chars().next().unwrap_or(' ');
    let indent = format!("{guide}    ");
    push_wrapped(lines, &format!("{indent}Why  "), &why_text(target), width);
    push_wrapped(
        lines,
        &format!("{indent}Do    "),
        &action_text(target, suggestions),
        width,
    );
    let mut boundary = location_text(target);
    if let Some(constraint) = target.constraints.first() {
        boundary.push_str(" · Keep: ");
        boundary.push_str(constraint);
    }
    push_wrapped(lines, &indent, &boundary, width);
}

fn location_text(target: &RefactorTarget) -> String {
    if target.kind == "module" {
        return "Module-wide".to_string();
    }
    let symbol = target.symbol.as_deref().unwrap_or("source");
    match (target.line_start, target.line_end) {
        (Some(start), Some(end)) if start != end => format!("{symbol} · lines {start}-{end}"),
        (Some(line), _) => format!("{symbol} · line {line}"),
        _ => format!("{symbol} · file-wide"),
    }
}

fn why_text(target: &RefactorTarget) -> String {
    for key in ["interpretation", "snippet"] {
        if let Some(value) = target.evidence.get(key).and_then(|value| value.as_str()) {
            if !value.is_empty() {
                return value.to_string();
            }
        }
    }
    match (target.current_value, target.threshold) {
        (Some(value), Some(threshold)) => format!(
            "{} measured {}; gate boundary {}.",
            target.metric,
            format_number(value),
            format_number(threshold)
        ),
        _ => format!("{} needs attention.", target.metric),
    }
}

fn action_text(target: &RefactorTarget, suggestions: &[Suggestion]) -> String {
    if let Some(suggestion) = suggestions.iter().find(|suggestion| {
        suggestion.metric.as_deref() == Some(target.metric.as_str())
            && suggestion.severity == target.severity
    }) {
        return suggestion.message.clone();
    }
    let operations: Vec<String> = target
        .recommended_operations
        .iter()
        .map(|operation| operation.replace('_', " "))
        .collect();
    if operations.is_empty() {
        "Inspect this metric and make the smallest behavior-preserving change.".to_string()
    } else {
        format!("Start with {}.", operations.join(" or "))
    }
}

fn format_number(value: f64) -> String {
    let formatted = format!("{value:.2}");
    formatted
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string()
}

fn push_wrapped(lines: &mut Vec<String>, prefix: &str, text: &str, width: usize) {
    let continuation = format!(
        "{}{}",
        prefix.chars().next().unwrap_or(' '),
        " ".repeat(prefix.chars().count().saturating_sub(1))
    );
    let available = width.saturating_sub(prefix.chars().count()).max(12);
    let mut current = String::new();
    let mut first = true;
    for word in text.split_whitespace() {
        let word = truncate_right(word, available);
        if !current.is_empty() && current.chars().count() + 1 + word.chars().count() > available {
            lines.push(format!(
                "{}{current}",
                if first { prefix } else { &continuation }
            ));
            current.clear();
            first = false;
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(&word);
    }
    if !current.is_empty() {
        lines.push(format!(
            "{}{current}",
            if first { prefix } else { &continuation }
        ));
    }
}

fn weakest_dimension(result: &ClassificationResult) -> (&'static str, f64) {
    Generator::ALL
        .map(Generator::as_str)
        .into_iter()
        .filter_map(|pillar| result.scores.get(pillar).map(|score| (pillar, *score)))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .unwrap_or(("unmeasured", 0.0))
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, HashMap};

    use topos_engine::core::omega::EvaluationValue;

    use super::*;

    fn result(simple: f64, secure: f64) -> ClassificationResult {
        ClassificationResult {
            scores: BTreeMap::from([
                ("simple".to_string(), simple),
                ("secure".to_string(), secure),
            ]),
            lattice_element: EvaluationValue::Slop,
            ..Default::default()
        }
    }

    #[test]
    fn location_text_uses_exact_span() {
        let target = RefactorTarget {
            target_id: "t".to_string(),
            kind: "function".to_string(),
            filepath: "a.rs".to_string(),
            symbol: Some("f".to_string()),
            line_start: Some(10),
            line_end: Some(24),
            failing_generators: vec!["simple".to_string()],
            metric: "ast.max_function_complexity".to_string(),
            current_value: Some(20.0),
            threshold: Some(15.0),
            severity: "fix".to_string(),
            recommended_operations: vec!["extract_helper".to_string()],
            constraints: Vec::new(),
            evidence: HashMap::new(),
        };
        assert_eq!(location_text(&target), "f · lines 10-24");
    }

    #[test]
    fn detail_uses_actionable_prose_and_module_location() {
        let target = RefactorTarget {
            target_id: "t".to_string(),
            kind: "module".to_string(),
            filepath: "a.rs".to_string(),
            symbol: Some("<module>".to_string()),
            line_start: Some(1),
            line_end: None,
            failing_generators: vec!["composable".to_string()],
            metric: "mdg.instability".to_string(),
            current_value: Some(0.18),
            threshold: Some(0.3),
            severity: "fix".to_string(),
            recommended_operations: vec!["rebalance_dependencies".to_string()],
            constraints: vec!["preserve module API".to_string()],
            evidence: HashMap::from([(
                "interpretation".to_string(),
                serde_json::Value::String(
                    "instability (0.18) is too low (module is too stable)".to_string(),
                ),
            )]),
        };
        let details = FileDetails {
            targets: vec![target],
            suggestions: vec![Suggestion {
                pillar: "composable".to_string(),
                metric: Some("mdg.instability".to_string()),
                severity: "fix".to_string(),
                message: "Rebalance dependencies (instability 0.18; aim for 0.3–0.7).".to_string(),
            }],
        };
        let output = detail_lines(
            Path::new("a.rs"),
            &result(1.0, 1.0),
            &details,
            72,
            false,
            true,
            1,
            5,
        )
        .join("\n");

        assert!(output.contains("esc back · q close"));
        assert!(output.contains("Why  instability (0.18) is too low"));
        assert!(output.contains("Do    Rebalance dependencies"));
        assert!(output.contains("Module-wide · Keep: preserve module API"));
        assert!(!output.contains("0 > 0"));
        assert!(output.lines().all(|line| line.chars().count() <= 72));
    }

    #[test]
    fn unguided_recommendations_do_not_grow_a_guide_when_wrapped() {
        let details = FileDetails {
            targets: vec![RefactorTarget {
                target_id: "t".to_string(),
                kind: "module".to_string(),
                filepath: "a.rs".to_string(),
                symbol: None,
                line_start: Some(1),
                line_end: None,
                failing_generators: vec!["simple".to_string()],
                metric: "cfg.cyclomatic".to_string(),
                current_value: Some(20.0),
                threshold: Some(15.0),
                severity: "fix".to_string(),
                recommended_operations: vec!["extract_helper".to_string()],
                constraints: vec!["preserve public behavior across every caller".to_string()],
                evidence: HashMap::new(),
            }],
            suggestions: Vec::new(),
        };
        let output = recommendation_lines(&details, 32, false, false).join("\n");
        assert!(!output.lines().any(|line| line.starts_with('│')));
    }
}
