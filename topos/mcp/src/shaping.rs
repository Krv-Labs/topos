//! Response-shaping helpers for the agent-facing summaries.
//!
//! The full documents the tools compute are too large to hand an agent
//! whole, so a tool answers with a summary instead. The helpers here are
//! its rules: floats to three significant figures (more where three would
//! make two different values look equal), a `bound` that says which side of
//! a limit a value is on, text clipped to a length, one `Omitted:` line that
//! counts what was left out, and one description per metric shown.

use std::collections::BTreeMap;

// Until #490 moves the descriptions onto the engine, they live with the PR
// recap gates.
use crate::pr_recap::gates::metric_description;

/// Figures kept by default.
const DEFAULT_FIGURES: u32 = 3;
/// Most figures [`round_apart`] will keep to tell two values apart.
const MAX_FIGURES: u32 = 6;

/// `value` to `figures` significant figures; zero and non-finite values
/// pass through.
pub fn sig(value: f64, figures: u32) -> f64 {
    if value == 0.0 || !value.is_finite() || figures == 0 {
        return value;
    }
    let decimals = (figures - 1) as usize;
    format!("{value:.decimals$e}").parse().unwrap_or(value)
}

/// `value` to three significant figures.
pub fn sig3(value: f64) -> f64 {
    sig(value, DEFAULT_FIGURES)
}

/// `values` to three significant figures, or to as many as four to six
/// when three would make two different values equal (a `9.9996` against a
/// `10` limit), else as they are. Non-finite values pass through.
pub fn round_apart(values: &[f64]) -> Vec<f64> {
    for figures in DEFAULT_FIGURES..=MAX_FIGURES {
        let rounded: Vec<f64> = values.iter().map(|&x| sig(x, figures)).collect();
        if !collides(values, &rounded) {
            return rounded;
        }
    }
    values.to_vec()
}

/// Whether rounding made two values equal that were not.
fn collides(raw: &[f64], rounded: &[f64]) -> bool {
    (0..raw.len()).any(|i| (i + 1..raw.len()).any(|j| raw[i] != raw[j] && rounded[i] == rounded[j]))
}

/// Which side of `limit` the value is on: `min` when it is below, `max`
/// when above. `None` when they are equal or either is not finite.
pub fn bound(value: f64, limit: f64) -> Option<&'static str> {
    if !value.is_finite() || !limit.is_finite() {
        return None;
    }
    match value.partial_cmp(&limit)? {
        std::cmp::Ordering::Less => Some("min"),
        std::cmp::Ordering::Greater => Some("max"),
        std::cmp::Ordering::Equal => None,
    }
}

/// `text` cut to `max_chars` characters, with `…` marking the cut.
pub fn clip(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let kept: String = text.chars().take(max_chars).collect();
    format!("{}…", kept.trim_end())
}

/// For `skip_serializing_if`: leave a zero count out of the document.
pub fn is_zero(count: &usize) -> bool {
    *count == 0
}

/// The markdown line that counts what a summary left out: `Omitted:`, each
/// non-zero count, then `more` (how to widen the summary). `None` when
/// nothing was omitted and there is no `more`.
pub fn omitted_line(counts: &[(&str, usize)], more: Option<&str>) -> Option<String> {
    let counted: Vec<String> = counts
        .iter()
        .filter(|(_, count)| *count > 0)
        .map(|(name, count)| format!("{name} {count}"))
        .collect();
    match (counted.is_empty(), more) {
        (true, None) => None,
        (true, Some(more)) => Some(more.to_string()),
        (false, None) => Some(format!("Omitted: {}.", counted.join(", "))),
        (false, Some(more)) => Some(format!("Omitted: {}. {more}", counted.join(", "))),
    }
}

/// One descriptive line per metric in `metrics` that has one.
// Follow-up: read the descriptions off the engine's `GateSpec` instead of
// `pr_recap::gates`, so a new gating metric brings its own.
pub fn describe_metrics<'a>(
    metrics: impl IntoIterator<Item = &'a str>,
) -> BTreeMap<String, &'static str> {
    metrics
        .into_iter()
        .filter_map(|metric| Some((metric.to_string(), metric_description(metric)?)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sig_keeps_figures_and_passes_zero_and_non_finite_through() {
        assert_eq!(sig(12.3456, 3), 12.3);
        assert_eq!(sig(0.0123456, 2), 0.012);
        assert_eq!(sig(1234.5, 3), 1230.0);
        assert_eq!(sig(-0.5556, 3), -0.556);
        assert_eq!(sig(0.0, 3), 0.0);
        assert!(sig(f64::NAN, 3).is_nan());
        assert_eq!(sig(f64::INFINITY, 3), f64::INFINITY);
        assert_eq!(sig3(99.96), 100.0);
    }

    #[test]
    fn round_apart_escalates_only_to_keep_distinct_values_distinct() {
        assert_eq!(round_apart(&[12.3456, 7.0]), vec![12.3, 7.0]);
        assert_eq!(round_apart(&[9.9996, 10.0]), vec![9.9996, 10.0]);
        assert_eq!(round_apart(&[9.99, 10.0]), vec![9.99, 10.0]);
        assert_eq!(round_apart(&[1.0000001, 1.0]), vec![1.0000001, 1.0]);
        assert_eq!(round_apart(&[2.0, 2.0]), vec![2.0, 2.0]);
        let mixed = round_apart(&[f64::NAN, 1.23456]);
        assert!(mixed[0].is_nan());
        assert_eq!(mixed[1], 1.23);
        assert!(round_apart(&[]).is_empty());
    }

    #[test]
    fn bound_names_the_side_and_says_nothing_when_there_is_none() {
        assert_eq!(bound(9.0, 10.0), Some("min"));
        assert_eq!(bound(11.0, 10.0), Some("max"));
        assert_eq!(bound(10.0, 10.0), None);
        assert_eq!(bound(f64::NAN, 10.0), None);
        assert_eq!(bound(1.0, f64::INFINITY), None);
    }

    #[test]
    fn clip_marks_the_cut_and_leaves_short_text_alone() {
        assert_eq!(clip("short", 10), "short");
        assert_eq!(clip("abcdef", 3), "abc…");
        assert_eq!(clip("ab  def", 3), "ab…");
        assert_eq!(clip("héllo wörld", 5), "héllo…");
    }

    #[test]
    fn is_zero_matches_skip_serializing_if() {
        assert!(is_zero(&0));
        assert!(!is_zero(&3));
    }

    #[test]
    fn omitted_line_skips_zero_counts_and_appends_more() {
        assert_eq!(omitted_line(&[("files", 0)], None), None);
        assert_eq!(
            omitted_line(&[("files", 2), ("skipped", 0), ("findings", 1)], None).unwrap(),
            "Omitted: files 2, findings 1."
        );
        assert_eq!(
            omitted_line(&[("files", 2)], Some("Pass paths.")).unwrap(),
            "Omitted: files 2. Pass paths."
        );
        assert_eq!(
            omitted_line(&[], Some("Pass paths.")).unwrap(),
            "Pass paths."
        );
    }

    #[test]
    fn describe_metrics_names_only_the_metrics_it_can_describe() {
        let described = describe_metrics(["ast.max_function_complexity", "no.such.metric"]);
        assert_eq!(described.len(), 1);
        assert!(described["ast.max_function_complexity"].contains("cyclomatic"));
    }
}
