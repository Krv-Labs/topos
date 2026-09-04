//! Pure human-readable rendering for `topos inspect`.

use console::Style;
use topos_engine::core::characteristic_morphism::ClassificationResult;
use topos_engine::functors::probes::ast::complexity::FunctionComplexityEntry;

use crate::commands::evaluate::info_render::{recommendation_lines, FileDetails};
use crate::commands::render::{paint, truncate_right, wrap_text, RenderOptions};

pub(super) fn inspection_detail_lines(
    result: &ClassificationResult,
    functions: &[FunctionComplexityEntry],
    details: &FileDetails,
    options: RenderOptions,
) -> Vec<String> {
    let mut lines = Vec::new();
    if !details.targets.is_empty() {
        lines.push(String::new());
        lines.extend(recommendation_lines(
            details,
            options.width,
            options.styled,
            false,
        ));
    }
    push_functions(&mut lines, functions, options);
    push_metrics(&mut lines, result, options);
    lines
}

fn push_functions(
    lines: &mut Vec<String>,
    functions: &[FunctionComplexityEntry],
    options: RenderOptions,
) {
    lines.push(String::new());
    lines.push(paint("Functions", Style::new().cyan().bold(), options));
    lines.push(String::new());
    if functions.is_empty() {
        lines.push(paint(
            "  No functions measured.",
            Style::new().dim(),
            options,
        ));
        return;
    }

    let shown = functions.len().min(10);
    let name_width = options.width.saturating_sub(30).clamp(20, 52);
    lines.push(paint(
        format!(
            "  {:<name_width$}  {:>11}  {:>10}",
            "FUNCTION", "LINES", "COMPLEXITY"
        ),
        Style::new().bold().dim(),
        options,
    ));
    for function in functions.iter().take(shown) {
        let location = if function.start_line == function.end_line {
            function.start_line.to_string()
        } else {
            format!("{}–{}", function.start_line, function.end_line)
        };
        lines.push(format!(
            "  {:<name_width$}  {:>11}  {:>10}",
            truncate_right(&function.qualified_name, name_width),
            location,
            function.complexity,
        ));
    }
    let suffix = if shown == functions.len() {
        format!(
            "{} function{} measured",
            functions.len(),
            plural(functions.len())
        )
    } else {
        format!("showing {shown} of {} functions", functions.len())
    };
    lines.push(paint(format!("  {suffix}"), Style::new().dim(), options));
}

fn push_metrics(lines: &mut Vec<String>, result: &ClassificationResult, options: RenderOptions) {
    lines.push(String::new());
    lines.push(paint("Metrics", Style::new().cyan().bold(), options));
    let mut interpreted: Vec<_> = result
        .raw_metrics
        .keys()
        .filter(|key| result.interpretation.contains_key(*key))
        .collect();
    let mut supporting: Vec<_> = result
        .raw_metrics
        .keys()
        .filter(|key| !result.interpretation.contains_key(*key))
        .collect();
    interpreted.sort();
    supporting.sort();

    if !interpreted.is_empty() {
        lines.push(String::new());
        lines.push(paint("  POLICY", Style::new().bold().dim(), options));
        for key in interpreted {
            push_metric(
                lines,
                key,
                result.raw_metrics[key],
                result.interpretation.get(key).map(String::as_str),
                options,
            );
        }
    }
    if !supporting.is_empty() {
        lines.push(String::new());
        lines.push(paint("  SUPPORTING", Style::new().bold().dim(), options));
        for key in supporting {
            push_metric(lines, key, result.raw_metrics[key], None, options);
        }
    }
}

fn push_metric(
    lines: &mut Vec<String>,
    key: &str,
    value: f64,
    interpretation: Option<&str>,
    options: RenderOptions,
) {
    let key_width = options.width.saturating_sub(50).clamp(22, 32);
    let prefix = format!(
        "  {:<key_width$}  {:>8.3}",
        truncate_right(key, key_width),
        value
    );
    let Some(interpretation) = interpretation else {
        lines.push(prefix);
        return;
    };
    let available = options
        .width
        .saturating_sub(prefix.chars().count() + 2)
        .max(16);
    let chunks = wrap_text(interpretation, available);
    if let Some(first) = chunks.first() {
        lines.push(format!(
            "{prefix}  {}",
            paint(first, Style::new().dim(), options)
        ));
    }
    let continuation = " ".repeat(prefix.chars().count() + 2);
    for chunk in chunks.iter().skip(1) {
        lines.push(format!(
            "{continuation}{}",
            paint(chunk, Style::new().dim(), options)
        ));
    }
}

fn plural(count: usize) -> &'static str {
    if count == 1 {
        ""
    } else {
        "s"
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use topos_engine::core::omega::EvaluationValue;

    use super::*;

    #[test]
    fn inspection_keeps_all_metrics_without_duplicate_entropy_section() {
        let result = ClassificationResult {
            raw_metrics: BTreeMap::from([
                ("ast.entropy".to_string(), 0.33),
                ("cfg.longest_path".to_string(), 8.0),
            ]),
            interpretation: BTreeMap::from([(
                "ast.entropy".to_string(),
                "entropy within structured range".to_string(),
            )]),
            lattice_element: EvaluationValue::Simple,
            ..Default::default()
        };
        let details = FileDetails {
            targets: Vec::new(),
            suggestions: Vec::new(),
        };
        let output = inspection_detail_lines(
            &result,
            &[],
            &details,
            RenderOptions {
                styled: false,
                width: 100,
            },
        )
        .join("\n");
        assert!(output.contains("ast.entropy"));
        assert!(output.contains("cfg.longest_path"));
        assert!(!output.contains("Entropy Analysis"));
    }
}
