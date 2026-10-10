//! Refactor-suggestion engine — turns a score into actionable next steps.
//!
//! Maps the metrics that *failed* their policy gate (and any active
//! security findings) into concrete, imperative, refactor-focused
//! instructions an agent or developer can act on directly. Gate decisions
//! come from [`crate::evaluation::policies::gates`] — the same specs the
//! scorers consult — so a suggestion can never fire on a gate the scorer
//! passed (including the entrypoint-module exemptions). Security prose
//! comes from [`crate::evaluation::security_guidance`].
//!
//! Advisory metrics (`cfg.cyclomatic`, `mdg.instability`, …) are not gates.
//! When [`crate::evaluation::advisory`] flags a reading as atypical for its
//! codebase, it yields a non-gating `"improve"` suggestion quoting the
//! relative percentile; [`advisory_operations`] names the refactor
//! operations that address it.
//!
//! Pure and side-effect-free so both the CLI and any future MCP layer can
//! render the same suggestions.
//!
//! Note SECURE suggestions only ever come from `active_findings`, never
//! from a failed `cpg.*` gate directly (unlike SIMPLE/COMPOSABLE, which
//! read straight off [`crate::evaluation::policies::gates::evaluate_gates`]).
//! A security suggestion needs the specific callee/line a finding carries
//! to be actionable; a bare gate failure has neither. This is a deliberate
//! asymmetry in the Python original, preserved here.

use crate::core::characteristic_morphism::ClassificationResult;
use crate::core::omega::{EvaluationValue, Generator};
use crate::evaluation::advisory::{AdvisoryReading, ADVISORY_METRICS};
use crate::evaluation::policies::gates::{
    binding_failure, evaluate_gates, GateOutcome, GateResult,
};
use crate::evaluation::preferences::{default_preferences, UserPreferences};
use crate::evaluation::security_guidance::{remediation_for, SecurityFinding};

/// One actionable, refactor-focused next step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    /// `"simple"` | `"composable"` | `"secure"` | `"coverage"`.
    pub pillar: String,
    /// Raw-metric key, or `None` for a finding/guidance-derived suggestion.
    pub metric: Option<String>,
    /// `"fix"` (a gate failed, or a security finding is active) |
    /// `"improve"` (an advisory metric is atypical for its codebase).
    pub severity: String,
    /// Imperative instruction.
    pub message: String,
}

/// Refactor operations addressing a flagged advisory metric (empty for a
/// metric with no advisory remedy).
pub fn advisory_operations(metric: &str) -> &'static [&'static str] {
    match metric {
        "cfg.cyclomatic" => &["extract_helper", "split_decision_logic"],
        "cfg.nesting_depth" => &["extract_helper"],
        "cfg.essential" => &["split_decision_logic"],
        "mdg.fan_in" => &["split_module"],
        "mdg.instability" => &["rebalance_dependencies", "extract_boundary"],
        _ => &[],
    }
}

/// Build actionable suggestions from a classification result.
///
/// `active_findings` are the security findings that are NOT allowlisted;
/// only these produce SECURE suggestions.
pub fn suggest_refactors(
    result: &ClassificationResult,
    active_findings: &[SecurityFinding],
    prefs: Option<&UserPreferences>,
) -> Vec<Suggestion> {
    if !result.is_parseable {
        return vec![Suggestion {
            pillar: "simple".to_string(),
            metric: None,
            severity: "fix".to_string(),
            message: "Fix the parse error so the file can be evaluated.".to_string(),
        }];
    }

    let default_prefs = default_preferences();
    let prefs = prefs.unwrap_or(&default_prefs);
    let current = result.lattice_element;

    // Same gate inputs and entrypoint exemption the scorers used, so a
    // suggestion can never fire on a gate the scorer passed.
    let gate_results = evaluate_gates(&result.raw_metrics, None, result.is_entrypoint_module);
    let failing: Vec<&GateResult> = gate_results
        .iter()
        .filter(|r| !r.passed() && r.spec.pillar != "secure")
        .collect();

    let mut ranked: Vec<RankedSuggestion> = failing
        .iter()
        .map(|r| {
            ranked_fix(
                prefs,
                current,
                result,
                &gate_results,
                r.spec.pillar,
                r.desirability(),
                Suggestion {
                    pillar: r.spec.pillar.to_string(),
                    metric: Some(r.spec.metric.to_string()),
                    severity: "fix".to_string(),
                    message: gate_message(r),
                },
            )
        })
        .collect();

    let secure_binding = binding_failure(&gate_results, "secure").map(|r| r.desirability());
    for finding in active_findings {
        ranked.push(ranked_fix(
            prefs,
            current,
            result,
            &gate_results,
            "secure",
            secure_binding.unwrap_or(0.0),
            Suggestion {
                pillar: "secure".to_string(),
                metric: finding.callee.clone(),
                severity: "fix".to_string(),
                message: remediation_for(finding).0,
            },
        ));
    }

    // Advisory suggestions trail every gating one: they cannot fail a
    // pillar, and agents act on `suggestions[0]`.
    for (_, metric, _) in ADVISORY_METRICS {
        if let Some(reading) = result.advisories.get(*metric).filter(|r| r.flagged) {
            ranked.push(RankedSuggestion {
                key: rank_key(prefs, current, reading.pillar, 1, 0, scale(reading.quality)),
                suggestion: Suggestion {
                    pillar: reading.pillar.to_string(),
                    metric: Some((*metric).to_string()),
                    severity: "improve".to_string(),
                    message: advisory_message(metric, reading),
                },
            });
        }
    }

    ranked.sort_by_key(|item| item.key);
    ranked.into_iter().map(|item| item.suggestion).collect()
}

struct RankedSuggestion {
    key: (usize, u8, i64, i64, usize),
    suggestion: Suggestion,
}

fn ranked_fix(
    prefs: &UserPreferences,
    current: EvaluationValue,
    result: &ClassificationResult,
    gates: &[GateResult],
    pillar: &str,
    desirability: f64,
    suggestion: Suggestion,
) -> RankedSuggestion {
    RankedSuggestion {
        key: rank_key(
            prefs,
            current,
            pillar,
            0,
            -scale(pillar_gate_score(result, gates, pillar)),
            scale(desirability),
        ),
        suggestion,
    }
}

/// `(outside goal, improve-after-fix, furthest pillar last, worst detail
/// first, preference rank)`.
fn rank_key(
    prefs: &UserPreferences,
    current: EvaluationValue,
    pillar: &str,
    tier: u8,
    neg_gate_score: i64,
    detail: i64,
) -> (usize, u8, i64, i64, usize) {
    let outside = match named_generator(pillar) {
        Some(generator) if prefs.goal_requires(current, generator) => 0,
        _ => 1,
    };
    let rank = prefs
        .ranking()
        .iter()
        .position(|generator| generator.as_str() == pillar)
        .unwrap_or(Generator::ALL.len());
    (outside, tier, neg_gate_score, detail, rank)
}

fn pillar_gate_score(result: &ClassificationResult, gates: &[GateResult], pillar: &str) -> f64 {
    if let Some(&score) = result.gate_scores.get(pillar) {
        return score;
    }
    binding_failure(gates, pillar)
        .map(|gate| gate.desirability())
        .unwrap_or(0.0)
}

fn named_generator(pillar: &str) -> Option<Generator> {
    Generator::ALL
        .into_iter()
        .find(|generator| generator.as_str() == pillar)
}

fn scale(value: f64) -> i64 {
    (value * 10_000.0).round() as i64
}

/// Imperative prose for a failed gate, quoting the real bounds.
fn gate_message(r: &GateResult) -> String {
    let value = r.value;
    let threshold = r.threshold().unwrap_or(value);
    match r.spec.metric {
        "ast.max_function_complexity" => format!(
            "Split the most complex function (complexity {value:.0} > {threshold:.0})."
        ),
        "ast.entropy" => {
            if r.outcome == GateOutcome::FailLow {
                format!("Consolidate repetitive/boilerplate code (entropy {value:.2} < {threshold}).")
            } else {
                format!("Decompose dense logic into named steps (entropy {value:.2} > {threshold}).")
            }
        }
        "mdg.fan_out" => format!(
            "Reduce fan-out {value:.0} (> {threshold:.0}) — introduce an interface or invert the dependency."
        ),
        "nav.max_function_divergence" => format!(
            "Flatten the deepest nested block (divergence {value:.1} > {threshold:.1})."
        ),
        _ => format!(
            "Bring {} ({value:.2}) inside its gate ({threshold:.2}).",
            r.spec.metric
        ),
    }
}

/// Imperative prose for a flagged advisory reading, quoting its relative
/// percentile (codebase + language prior).
fn advisory_message(metric: &str, reading: &AdvisoryReading) -> String {
    let value = reading.value;
    let action = match metric {
        "cfg.cyclomatic" => {
            format!("Collapse redundant decisions or split this file (cyclomatic {value:.0})")
        }
        "cfg.nesting_depth" => {
            format!("Extract the deepest nested block into a helper (nesting depth {value:.0})")
        }
        "cfg.essential" => format!(
            "Split tangled decision logic into structured steps (essential complexity {value:.0})"
        ),
        "mdg.fan_in" => format!(
            "Review this file's responsibility (fan-in {value:.0}); consider splitting the module"
        ),
        "mdg.instability" => format!("Rebalance dependencies (instability {value:.2})"),
        _ => format!("Review {metric} ({value:.2})"),
    };
    format!(
        "{action} — atypical for this codebase (relative percentile {:.0}). Advisory: does not gate the pillar.",
        reading.relative_percentile * 100.0
    )
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::core::omega::EvaluationValue;
    use crate::evaluation::policies::base::Priority;

    fn result(
        dimensions: BTreeMap<String, EvaluationValue>,
        raw_metrics: BTreeMap<String, f64>,
        lattice_element: EvaluationValue,
    ) -> ClassificationResult {
        ClassificationResult {
            is_parseable: true,
            dimensions,
            scores: BTreeMap::new(),
            lattice_element,
            priority: Priority::Secure,
            raw_metrics,
            interpretation: BTreeMap::new(),
            is_entrypoint_module: false,
            is_stable_leaf_module: false,
            ..Default::default()
        }
    }

    fn reading(
        pillar: &'static str,
        value: f64,
        percentile: f64,
        flagged: bool,
    ) -> AdvisoryReading {
        AdvisoryReading {
            pillar,
            value,
            relative_percentile: percentile,
            global_percentile: percentile,
            local_weight: 0.0,
            quality: 1.0 - percentile,
            flagged,
        }
    }

    #[test]
    fn eval_finding_yields_secure_fix_naming_callee() {
        let result = result(
            BTreeMap::from([("secure".to_string(), EvaluationValue::Slop)]),
            BTreeMap::from([
                ("cpg.dangerous_calls".to_string(), 1.0),
                ("cpg.taint_flows".to_string(), 0.0),
            ]),
            EvaluationValue::Slop,
        );
        let finding = SecurityFinding {
            kind: "dangerous_call".to_string(),
            line: 2,
            snippet: "return eval(x)".to_string(),
            callee: Some("eval".to_string()),
            source: None,
            sink: None,
        };

        let suggestions = suggest_refactors(&result, &[finding], None);
        let secure: Vec<&Suggestion> = suggestions
            .iter()
            .filter(|s| s.pillar == "secure")
            .collect();
        assert!(
            !secure.is_empty(),
            "expected a SECURE suggestion for an eval finding"
        );
        assert_eq!(secure[0].severity, "fix");
        assert!(secure[0].message.contains("eval"));
    }

    /// Pick a suggestion by metric key -- never by index.
    fn by_metric<'a>(suggestions: &'a [Suggestion], metric: &str) -> &'a Suggestion {
        suggestions
            .iter()
            .find(|s| s.metric.as_deref() == Some(metric))
            .unwrap_or_else(|| panic!("expected a suggestion for {metric}"))
    }

    #[test]
    fn high_cyclomatic_alone_is_not_a_gate_failure() {
        // `cfg.cyclomatic` is advisory (issue #193) and no longer a gate:
        // without a flagged advisory reading it yields nothing.
        let result = result(
            BTreeMap::from([("simple".to_string(), EvaluationValue::Simple)]),
            BTreeMap::from([
                ("cfg.cyclomatic".to_string(), 25.0),
                ("ast.entropy".to_string(), 0.5),
            ]),
            EvaluationValue::Simple,
        );
        assert_eq!(suggest_refactors(&result, &[], None), vec![]);
    }

    #[test]
    fn flagged_advisory_yields_improve_suggestion_with_percentile() {
        let mut result = result(
            BTreeMap::from([("simple".to_string(), EvaluationValue::Simple)]),
            BTreeMap::from([
                ("cfg.cyclomatic".to_string(), 25.0),
                ("mdg.instability".to_string(), 0.4),
            ]),
            EvaluationValue::Simple,
        );
        result.advisories = BTreeMap::from([
            (
                "cfg.cyclomatic".to_string(),
                reading("simple", 25.0, 0.97, true),
            ),
            (
                "mdg.instability".to_string(),
                reading("composable", 0.4, 0.5, false),
            ),
        ]);

        let suggestions = suggest_refactors(&result, &[], None);
        assert_eq!(suggestions.len(), 1, "only flagged readings surface");
        let cyclomatic = by_metric(&suggestions, "cfg.cyclomatic");
        assert_eq!(cyclomatic.pillar, "simple");
        assert_eq!(cyclomatic.severity, "improve");
        assert!(cyclomatic.message.contains("atypical for this codebase"));
        assert!(cyclomatic.message.contains("relative percentile 97"));
        assert_eq!(
            advisory_operations("cfg.cyclomatic"),
            &["extract_helper", "split_decision_logic"]
        );
        assert_eq!(
            advisory_operations("mdg.instability"),
            &["rebalance_dependencies", "extract_boundary"]
        );
    }

    #[test]
    fn gating_suggestions_lead_advisory_ones() {
        let mut result = result(
            BTreeMap::from([("simple".to_string(), EvaluationValue::Slop)]),
            BTreeMap::from([
                ("cfg.cyclomatic".to_string(), 25.0),
                ("ast.max_function_complexity".to_string(), 20.0),
                ("ast.entropy".to_string(), 0.95),
            ]),
            EvaluationValue::Slop,
        );
        result.advisories = BTreeMap::from([(
            "cfg.cyclomatic".to_string(),
            reading("simple", 25.0, 0.99, true),
        )]);

        let suggestions = suggest_refactors(&result, &[], None);
        let order: Vec<&str> = suggestions.iter().map(|s| s.severity.as_str()).collect();
        assert_eq!(order, vec!["fix", "fix", "improve"]);
        assert_eq!(
            suggestions[0].metric.as_deref(),
            Some("ast.max_function_complexity")
        );
        assert_eq!(suggestions[1].metric.as_deref(), Some("ast.entropy"));
        assert_eq!(suggestions[2].metric.as_deref(), Some("cfg.cyclomatic"));
    }

    #[test]
    fn navigable_divergence_yields_a_fix() {
        let result = result(
            BTreeMap::from([("navigable".to_string(), EvaluationValue::Slop)]),
            BTreeMap::from([("nav.max_function_divergence".to_string(), 20.0)]),
            EvaluationValue::Slop,
        );
        let suggestions = suggest_refactors(&result, &[], None);
        let divergence = by_metric(&suggestions, "nav.max_function_divergence");
        assert_eq!(divergence.pillar, "navigable");
        assert_eq!(divergence.severity, "fix");
        assert!(divergence.message.contains("Flatten"));
    }

    #[test]
    fn closer_pillar_gate_leads() {
        let mut result = result(
            BTreeMap::from([
                ("simple".to_string(), EvaluationValue::Slop),
                ("navigable".to_string(), EvaluationValue::Slop),
            ]),
            BTreeMap::from([
                ("ast.max_function_complexity".to_string(), 20.0),
                ("nav.max_function_divergence".to_string(), 12.0),
            ]),
            EvaluationValue::Slop,
        );
        result.gate_scores =
            BTreeMap::from([("simple".to_string(), 0.1), ("navigable".to_string(), 0.4)]);
        let suggestions = suggest_refactors(&result, &[], None);
        assert_eq!(
            suggestions[0].metric.as_deref(),
            Some("nav.max_function_divergence")
        );
        assert_eq!(
            suggestions[1].metric.as_deref(),
            Some("ast.max_function_complexity")
        );
    }

    #[test]
    fn flagged_advisories_sort_worst_quality_first() {
        let mut result = result(
            BTreeMap::from([("simple".to_string(), EvaluationValue::Simple)]),
            BTreeMap::new(),
            EvaluationValue::Simple,
        );
        result.advisories = BTreeMap::from([
            (
                "cfg.cyclomatic".to_string(),
                reading("simple", 25.0, 0.91, true),
            ),
            (
                "cfg.nesting_depth".to_string(),
                reading("simple", 8.0, 0.99, true),
            ),
        ]);
        let suggestions = suggest_refactors(&result, &[], None);
        assert_eq!(suggestions[0].metric.as_deref(), Some("cfg.nesting_depth"));
        assert_eq!(suggestions[1].metric.as_deref(), Some("cfg.cyclomatic"));
    }

    #[test]
    fn high_fan_out_yields_composable_suggestion() {
        let result = result(
            BTreeMap::from([("composable".to_string(), EvaluationValue::Slop)]),
            BTreeMap::from([
                ("mdg.fan_out".to_string(), 30.0),
                ("mdg.instability".to_string(), 0.5),
            ]),
            EvaluationValue::Slop,
        );

        let suggestions = suggest_refactors(&result, &[], None);
        let fan_out = by_metric(&suggestions, "mdg.fan_out");
        assert_eq!(fan_out.severity, "fix");
        assert_eq!(suggestions.len(), 1, "instability is not a gate");
    }

    #[test]
    fn clean_file_yields_no_suggestions() {
        let result = result(
            BTreeMap::from([
                ("simple".to_string(), EvaluationValue::Simple),
                ("secure".to_string(), EvaluationValue::Secure),
            ]),
            BTreeMap::from([
                ("cfg.cyclomatic".to_string(), 2.0),
                ("ast.entropy".to_string(), 0.5),
                ("cpg.dangerous_calls".to_string(), 0.0),
                ("cpg.taint_flows".to_string(), 0.0),
            ]),
            EvaluationValue::Ideal,
        );

        assert_eq!(suggest_refactors(&result, &[], None), vec![]);
    }

    #[test]
    fn allowlisted_finding_produces_no_secure_suggestion() {
        // The CLI passes only NON-allowlisted findings as active_findings.
        let result = result(
            BTreeMap::from([("secure".to_string(), EvaluationValue::Slop)]),
            BTreeMap::from([
                ("cpg.dangerous_calls".to_string(), 1.0),
                ("cpg.taint_flows".to_string(), 0.0),
            ]),
            EvaluationValue::Secure,
        );

        let suggestions = suggest_refactors(&result, &[], None);
        assert!(!suggestions.iter().any(|s| s.pillar == "secure"));
    }
}
