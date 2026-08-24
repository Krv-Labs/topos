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

use crate::core::characteristic_morphism::ClassificationResult;
use crate::evaluation::policies::composable::coupling_gate_input;
use crate::evaluation::policies::gates::{evaluate_gates, GateOutcome, GateResult};
use crate::evaluation::security_guidance::{remediation_for, SecurityFinding};

/// One actionable, refactor-focused next step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    /// `"simple"` | `"composable"` | `"secure"` | `"coverage"`.
    pub pillar: String,
    /// Raw-metric key, or `None` for a finding/guidance-derived suggestion.
    pub metric: Option<String>,
    /// `"fix"` (a pillar-gating gate failed, or a security finding is
    /// active) | `"improve"` (an advisory, non-pillar-gating gate failed).
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
    "mdg.main_sequence_distance",
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

    // Reproduce the exact gate inputs each scorer used, so a suggestion
    // can never fire on a gate the scorer passed: COMPOSABLE swaps raw
    // instability for `mdg.main_sequence_distance` in distance mode (shared
    // with Φ_COMPOSABLE via `coupling_gate_input`), and the stable-leaf and
    // instability exemptions are threaded through rather than hard-coded.
    let instability = result.raw_metrics.get("mdg.instability").copied();
    let mut gate_metrics: HashMap<String, f64> = result
        .raw_metrics
        .iter()
        .map(|(k, v)| (k.clone(), *v))
        .collect();
    gate_metrics.remove("mdg.instability");
    gate_metrics.extend(coupling_gate_input(
        instability,
        result.raw_metrics.get("mdg.fan_in").copied(),
        result.raw_metrics.get("mdg.fan_out").copied(),
        result.raw_metrics.get("mdg.abstractness").copied(),
        result.raw_metrics.get("mdg.coupling").copied(),
    ));
    let gate_results = evaluate_gates(
        &gate_metrics,
        None,
        result.is_entrypoint_module,
        result.is_stable_leaf_module,
        instability,
    );
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
                // Severity tracks what the gate can actually cost you:
                // only a `gates_achieved` gate can drag its pillar's
                // `achieved` down, so only it warrants "fix". Advisory
                // gates (`cfg.cyclomatic` -- issue #193 -- plus
                // `mdg.instability` and `mdg.main_sequence_distance`,
                // whose file-level resolution is too coarse to gate) are
                // still worth acting on but cannot fail a pillar, so
                // telling an agent to "fix" them misdirects the loop.
                severity: if r.spec.gates_achieved {
                    "fix".to_string()
                } else {
                    "improve".to_string()
                },
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

    // Gating suggestions lead. `SUGGESTION_ORDER` opens with
    // `cfg.cyclomatic`, which is advisory (`gates_achieved: false`, issue
    // #193), so without this an agent reading `suggestions[0]` is pointed
    // at the one metric no verdict depends on -- the same misrouting that
    // `refactor_targets` ranks around in `topos-mcp`. Correcting only the
    // severity label was not enough: order is what a reader acts on. The
    // sort is stable, so `SUGGESTION_ORDER` still decides ties within a
    // tier and security findings stay behind the other gate failures.
    suggestions.sort_by_key(|s| usize::from(s.severity != "fix"));
    suggestions
}

/// Imperative prose for a failed gate, quoting the real bounds.
fn gate_message(r: &GateResult) -> String {
    let value = r.value;
    let threshold = r.threshold().unwrap_or(value);
    match r.spec.metric {
        "cfg.cyclomatic" => format!(
            "Collapse redundant decisions or split this file (cyclomatic {value:.0} > {threshold:.0}) — extracting helpers lowers `ast.max_function_complexity` but raises this whole-file sum."
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
        "mdg.main_sequence_distance" => format!(
            "Rebalance abstraction and dependencies (main-sequence distance {value:.2} > {threshold:.2})."
        ),
        "mdg.fan_out" => format!(
            "Reduce fan-out {value:.0} (> {threshold:.0}) — introduce an interface or invert the dependency."
        ),
        // mdg.fan_in
        _ => format!(
            "Review this file's responsibility (fan-in {value:.0} > {threshold:.0}); many external symbols call it."
        ),
    }
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

    /// Pick a suggestion by metric key -- never by index. `SUGGESTION_ORDER`
    /// puts the advisory `cfg.cyclomatic` first, so `suggestions[0]` is not
    /// the most severe entry.
    fn by_metric<'a>(suggestions: &'a [Suggestion], metric: &str) -> &'a Suggestion {
        suggestions
            .iter()
            .find(|s| s.metric.as_deref() == Some(metric))
            .unwrap_or_else(|| panic!("expected a suggestion for {metric}"))
    }

    #[test]
    fn high_cyclomatic_alone_yields_advisory_simple_suggestion() {
        // Only `cfg.cyclomatic` fails here: entropy 0.5 is in band and
        // `ast.max_function_complexity` is absent (unmeasured metrics are
        // skipped by `evaluate_gates`). Because cyclomatic is advisory
        // (`gates_achieved: false`, issue #193) it cannot fail SIMPLE, so
        // the suggestion must say "improve" rather than "fix".
        let result = result(
            BTreeMap::from([("simple".to_string(), EvaluationValue::Slop)]),
            BTreeMap::from([
                ("cfg.cyclomatic".to_string(), 25.0),
                ("ast.entropy".to_string(), 0.5),
            ]),
            EvaluationValue::Slop,
        );

        let suggestions = suggest_refactors(&result, &[]);
        let cyclomatic = by_metric(&suggestions, "cfg.cyclomatic");
        assert_eq!(cyclomatic.pillar, "simple");
        assert_eq!(cyclomatic.severity, "improve");
        assert!(cyclomatic.message.to_lowercase().contains("cyclomatic"));
    }

    #[test]
    fn severity_tracks_whether_the_gate_can_fail_its_pillar() {
        // One fixture failing all three SIMPLE gates, so the two severities
        // are pinned as coexisting rather than one clobbering the other.
        // `is_entrypoint_module: false` keeps the entropy exemption from
        // swallowing the entropy failure.
        let result = result(
            BTreeMap::from([("simple".to_string(), EvaluationValue::Slop)]),
            BTreeMap::from([
                ("cfg.cyclomatic".to_string(), 25.0),
                ("ast.max_function_complexity".to_string(), 20.0),
                ("ast.entropy".to_string(), 0.95),
            ]),
            EvaluationValue::Slop,
        );

        let suggestions = suggest_refactors(&result, &[]);
        // Advisory: high whole-file branching cannot fail SIMPLE.
        assert_eq!(
            by_metric(&suggestions, "cfg.cyclomatic").severity,
            "improve"
        );
        // Gating: these two do decide SIMPLE's `achieved`.
        assert_eq!(
            by_metric(&suggestions, "ast.max_function_complexity").severity,
            "fix"
        );
        assert_eq!(by_metric(&suggestions, "ast.entropy").severity, "fix");
    }

    #[test]
    fn gating_suggestions_lead_advisory_ones() {
        // `SUGGESTION_ORDER` opens with the advisory `cfg.cyclomatic`, so
        // before the tier sort an agent reading `suggestions[0]` was sent at
        // the one metric that cannot fail a pillar -- even once its severity
        // label was corrected. Same fixture as the severity test: all three
        // SIMPLE gates fail, and cyclomatic has by far the largest excess.
        let result = result(
            BTreeMap::from([("simple".to_string(), EvaluationValue::Slop)]),
            BTreeMap::from([
                ("cfg.cyclomatic".to_string(), 25.0),
                ("ast.max_function_complexity".to_string(), 20.0),
                ("ast.entropy".to_string(), 0.95),
            ]),
            EvaluationValue::Slop,
        );

        let suggestions = suggest_refactors(&result, &[]);
        let order: Vec<&str> = suggestions.iter().map(|s| s.severity.as_str()).collect();
        let first_advisory = order.iter().position(|s| *s == "improve");
        let last_gating = order.iter().rposition(|s| *s == "fix");
        assert!(
            matches!((first_advisory, last_gating), (Some(a), Some(g)) if g < a),
            "every gating suggestion must precede every advisory one, got {order:?}"
        );
        assert_eq!(
            suggestions[0].metric.as_deref(),
            Some("ast.max_function_complexity"),
            "the largest-excess metric is advisory cyclomatic; a real gate failure must still lead"
        );
        // Stability: within the gating tier, SUGGESTION_ORDER still decides.
        assert_eq!(suggestions[1].metric.as_deref(), Some("ast.entropy"));
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
        assert!(suggestions
            .iter()
            .any(|s| s.metric.as_deref() == Some("mdg.fan_out")));
    }

    #[test]
    fn main_sequence_failure_gets_actionable_composable_suggestion() {
        // Distance mode needs a nonzero abstractness reading *and* coupling
        // at or above the ratio's resolution limit (see `coupling_gate_input`),
        // so the fixture carries both: distance = |0.2 + 0.1 − 1| = 0.7.
        let result = result(
            BTreeMap::from([("composable".to_string(), EvaluationValue::Slop)]),
            BTreeMap::from([
                ("mdg.instability".to_string(), 0.1),
                ("mdg.abstractness".to_string(), 0.2),
                ("mdg.coupling".to_string(), 6.0),
                ("mdg.fan_in".to_string(), 1.0),
                ("mdg.fan_out".to_string(), 5.0),
            ]),
            EvaluationValue::Slop,
        );

        let suggestions = suggest_refactors(&result, &[]);
        let suggestion = by_metric(&suggestions, "mdg.main_sequence_distance");
        // Advisory: still surfaced and still actionable, but it cannot fail
        // COMPOSABLE on its own, so "improve" rather than "fix".
        assert_eq!(suggestion.severity, "improve");
        assert!(suggestion.message.contains("Rebalance abstraction"));
        assert!(suggestion.message.contains("0.70 > 0.50"));
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
