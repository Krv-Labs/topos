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

use crate::evaluation::characteristic_morphism::ClassificationResult;
use crate::evaluation::policies::gates::{evaluate_gates, GateOutcome, GateResult};
use crate::evaluation::security_guidance::{remediation_for, SecurityFinding};

/// One actionable, refactor-focused next step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    /// `"simple"` | `"composable"` | `"secure"` | `"coverage"`.
    pub pillar: String,
    /// Raw-metric key, or `None` for a finding/guidance-derived suggestion.
    pub metric: Option<String>,
    /// `"fix"` (gate failed) | `"improve"` (advisory).
    pub severity: String,
    /// Imperative instruction.
    pub message: String,
}

/// Legacy emission order (SIMPLE gates before COMPOSABLE, cyclomatic first).
const SUGGESTION_ORDER: &[&str] = &[
    "cfg.cyclomatic",
    "ast.max_function_complexity",
    "ast.entropy",
    "mdg.instability",
    "mdg.fan_out",
    "mdg.fan_in",
];

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
    suggestions
}

/// Imperative prose for a failed gate, quoting the real bounds.
fn gate_message(r: &GateResult) -> String {
    let value = r.value;
    let threshold = r.threshold().unwrap_or(value);
    match r.spec.metric {
        "cfg.cyclomatic" => format!(
            "Extract helper functions to cut branching (cyclomatic {value:.0} > {threshold:.0})."
        ),
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
        "mdg.instability" => format!(
            "Rebalance dependencies (instability {value:.2}; aim for {}–{}).",
            r.spec.low.unwrap_or(0.0),
            r.spec.high.unwrap_or(1.0)
        ),
        "mdg.fan_out" => format!(
            "Reduce fan-out {value:.0} (> {threshold:.0}) — introduce an interface or invert the dependency."
        ),
        // mdg.fan_in
        _ => format!(
            "Split this module (fan-in {value:.0} > {threshold:.0}); too many modules depend on it."
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::omega::EvaluationValue;
    use crate::evaluation::policies::base::Priority;

    fn result(
        dimensions: HashMap<String, EvaluationValue>,
        raw_metrics: HashMap<String, f64>,
        lattice_element: EvaluationValue,
    ) -> ClassificationResult {
        ClassificationResult {
            is_parseable: true,
            dimensions,
            scores: HashMap::new(),
            lattice_element,
            priority: Priority::Secure,
            raw_metrics,
            interpretation: HashMap::new(),
            is_entrypoint_module: false,
        }
    }

    #[test]
    fn eval_finding_yields_secure_fix_naming_callee() {
        let result = result(
            HashMap::from([("secure".to_string(), EvaluationValue::Slop)]),
            HashMap::from([
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

    #[test]
    fn high_cyclomatic_yields_simple_suggestion() {
        let result = result(
            HashMap::from([("simple".to_string(), EvaluationValue::Slop)]),
            HashMap::from([
                ("cfg.cyclomatic".to_string(), 25.0),
                ("ast.entropy".to_string(), 0.5),
            ]),
            EvaluationValue::Slop,
        );

        let suggestions = suggest_refactors(&result, &[]);
        let simple: Vec<&Suggestion> = suggestions
            .iter()
            .filter(|s| s.metric.as_deref() == Some("cfg.cyclomatic"))
            .collect();
        assert!(!simple.is_empty());
        assert_eq!(simple[0].severity, "fix");
        assert!(simple[0].message.to_lowercase().contains("cyclomatic"));
    }

    #[test]
    fn high_fan_out_yields_composable_suggestion() {
        let result = result(
            HashMap::from([("composable".to_string(), EvaluationValue::Slop)]),
            HashMap::from([
                ("mdg.fan_out".to_string(), 30.0),
                ("mdg.instability".to_string(), 0.5),
            ]),
            EvaluationValue::Slop,
        );

        let suggestions = suggest_refactors(&result, &[]);
        assert!(suggestions
            .iter()
            .any(|s| s.metric.as_deref() == Some("mdg.fan_out")));
    }

    #[test]
    fn clean_file_yields_no_suggestions() {
        let result = result(
            HashMap::from([
                ("simple".to_string(), EvaluationValue::Simple),
                ("secure".to_string(), EvaluationValue::Secure),
            ]),
            HashMap::from([
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
            HashMap::from([("secure".to_string(), EvaluationValue::Slop)]),
            HashMap::from([
                ("cpg.dangerous_calls".to_string(), 1.0),
                ("cpg.taint_flows".to_string(), 0.0),
            ]),
            EvaluationValue::Secure,
        );

        let suggestions = suggest_refactors(&result, &[]);
        assert!(!suggestions.iter().any(|s| s.pillar == "secure"));
    }
}
