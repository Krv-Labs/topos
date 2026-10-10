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

use std::collections::HashMap;

use crate::core::characteristic_morphism::ClassificationResult;
use crate::evaluation::advisory::{AdvisoryReading, ADVISORY_METRICS};
use crate::evaluation::policies::gates::{evaluate_gates, GateOutcome, GateResult};
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

/// Emission order for gate failures (SIMPLE before COMPOSABLE).
const SUGGESTION_ORDER: &[&str] = &["ast.max_function_complexity", "ast.entropy", "mdg.fan_out"];

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
) -> Vec<Suggestion> {
    if !result.is_parseable {
        return vec![Suggestion {
            pillar: "simple".to_string(),
            metric: None,
            severity: "fix".to_string(),
            message: "Fix the parse error so the file can be evaluated.".to_string(),
        }];
    }

    // Same gate inputs and entrypoint exemption the scorers used, so a
    // suggestion can never fire on a gate the scorer passed.
    let gate_results = evaluate_gates(&result.raw_metrics, None, result.is_entrypoint_module);
    let failing: HashMap<&str, &GateResult> = gate_results
        .iter()
        .filter(|r| !r.passed() && r.spec.pillar != "secure")
        .map(|r| (r.spec.metric, r))
        .collect();

    let mut suggestions: Vec<Suggestion> = SUGGESTION_ORDER
        .iter()
        .filter_map(|metric| {
            failing.get(metric).map(|r| Suggestion {
                pillar: r.spec.pillar.to_string(),
                metric: Some(metric.to_string()),
                severity: "fix".to_string(),
                message: gate_message(r),
            })
        })
        .collect();

    for finding in active_findings {
        suggestions.push(Suggestion {
            pillar: "secure".to_string(),
            metric: finding.callee.clone(),
            severity: "fix".to_string(),
            message: remediation_for(finding).0,
        });
    }

    // Advisory suggestions trail every gating one: they cannot fail a
    // pillar, and agents act on `suggestions[0]`. Emitted in
    // `ADVISORY_METRICS` order.
    for (_, metric, _) in ADVISORY_METRICS {
        if let Some(reading) = result.advisories.get(*metric).filter(|r| r.flagged) {
            suggestions.push(Suggestion {
                pillar: reading.pillar.to_string(),
                metric: Some(metric.to_string()),
                severity: "improve".to_string(),
                message: advisory_message(metric, reading),
            });
        }
    }
    suggestions
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
        // mdg.fan_out
        _ => format!(
            "Reduce fan-out {value:.0} (> {threshold:.0}) — introduce an interface or invert the dependency."
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

        let suggestions = suggest_refactors(&result, &[finding]);
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
        assert_eq!(suggest_refactors(&result, &[]), vec![]);
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

        let suggestions = suggest_refactors(&result, &[]);
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

        let suggestions = suggest_refactors(&result, &[]);
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
    fn high_fan_out_yields_composable_suggestion() {
        let result = result(
            BTreeMap::from([("composable".to_string(), EvaluationValue::Slop)]),
            BTreeMap::from([
                ("mdg.fan_out".to_string(), 30.0),
                ("mdg.instability".to_string(), 0.5),
            ]),
            EvaluationValue::Slop,
        );

        let suggestions = suggest_refactors(&result, &[]);
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

        assert_eq!(suggest_refactors(&result, &[]), vec![]);
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

        let suggestions = suggest_refactors(&result, &[]);
        assert!(!suggestions.iter().any(|s| s.pillar == "secure"));
    }
}
